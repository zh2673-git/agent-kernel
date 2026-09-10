//! 调度器（时间编排）：优先级泳道、背压（有界许可）、per-plugin 在途上限、
//! drain 等待（B4）。串行语义由执行域在 dispatch 内加锁实现，此处只管在途许可。
//!
//! 泳道（v0.1.7，T8 落地——此前 Priority 只有枚举、调度器不消费）：
//! - **System**：专用全量闸（`total` / `lanes.all`）——最高优先且不受 Normal 占用影响；
//! - **Normal**：专用闸（`total_normal` / `lanes.normal`），容量 = 全量 − 1（预留 1 个
//!   在途位给 System；全量=1 时退化为不预留，防饿死）。
//! 两套闸互不重叠：Normal 打满时 System 仍有余位即时受理（cancel 类短 op 的刚需）；
//! Normal 永远拿不到预留位（防反向饿死）。在途计数/drain 两条泳道合并核算。

use agent_kernel_core::{KernelError, Manifest, PluginId, PluginState, Priority};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

pub type SchedulerRef = Arc<Scheduler>;

/// 单插件双泳道闸。
#[derive(Clone)]
struct PluginLanes {
    /// System 泳道（全量容量）。
    all: Arc<Semaphore>,
    /// Normal 泳道（全量 − 1，预留 1 给 System；全量=1 时不预留）。
    normal: Arc<Semaphore>,
}

pub struct Scheduler {
    /// System 泳道全局闸（全量）。
    total: Arc<Semaphore>,
    /// Normal 泳道全局闸（全量 − 1，预留 1）。
    total_normal: Arc<Semaphore>,
    /// 每插件在途上限（manifest.max_inflight，B4）——双泳道各自成闸。
    per_plugin: Mutex<HashMap<PluginId, PluginLanes>>,
    /// 当前在途计数（drain 判定；两泳道合并）。
    inflight: Mutex<HashMap<PluginId, usize>>,
    /// drain 完成时唤醒等待者。
    notify: Arc<Notify>,
}

/// 在途许可守卫。Drop 时递减在途计数并通知（B4 drain 可见性）。
pub struct Guards {
    _permits: Vec<OwnedSemaphorePermit>,
    sched: SchedulerRef,
    id: PluginId,
}

impl Drop for Guards {
    fn drop(&mut self) {
        self.sched.release(&self.id);
    }
}

impl Scheduler {
    pub fn new(max_total: usize) -> SchedulerRef {
        let max_total = max_total.max(1);
        Arc::new(Self {
            total: Arc::new(Semaphore::new(max_total)),
            // 预留 1 给 System；全量=1 时不预留（1 也保证 Normal 不会饿死）
            total_normal: Arc::new(Semaphore::new(max_total.saturating_sub(1).max(1))),
            per_plugin: Mutex::new(HashMap::new()),
            inflight: Mutex::new(HashMap::new()),
            notify: Arc::new(Notify::new()),
        })
    }

    /// 获取在途许可。Draining ⇒ 退化为 Reject（K403，B4）。
    /// `priority` 决定泳道：System 走全量闸，Normal 走预留后闸（T8）。
    pub async fn acquire(
        self: Arc<Self>,
        id: &PluginId,
        manifest: &Manifest,
        state: PluginState,
        priority: Priority,
        deadline: Option<Duration>,
    ) -> Result<Guards, KernelError> {
        if state == PluginState::Draining {
            return Err(KernelError::DrainingReject(id.clone()));
        }
        let cap = manifest.max_inflight.unwrap_or(16).max(1);
        let lanes = self
            .per_plugin
            .lock()
            .unwrap()
            .entry(id.clone())
            .or_insert_with(|| PluginLanes {
                all: Arc::new(Semaphore::new(cap)),
                normal: Arc::new(Semaphore::new(if cap >= 2 { cap - 1 } else { cap })),
            })
            .clone();
        let (total_sem, lane_sem) = if priority == Priority::System {
            (self.total.clone(), lanes.all.clone())
        } else {
            (self.total_normal.clone(), lanes.normal.clone())
        };
        let total = acquire_owned(&total_sem, deadline).await?;
        let per = acquire_owned(&lane_sem, deadline).await?;
        *self.inflight.lock().unwrap().entry(id.clone()).or_insert(0) += 1;
        Ok(Guards {
            _permits: vec![total, per],
            sched: self,
            id: id.clone(),
        })
    }

    fn release(&self, id: &PluginId) {
        let mut g = self.inflight.lock().unwrap();
        if let Some(c) = g.get_mut(id) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                g.remove(id);
            }
        }
        self.notify.notify_waiters();
    }

    pub fn inflight_count(&self, id: &PluginId) -> usize {
        *self.inflight.lock().unwrap().get(id).unwrap_or(&0)
    }

    /// 等待在途请求 drain 完成（B4）。超时则强制继续卸载。
    pub async fn drain_wait(&self, id: &PluginId, timeout: Duration) {
        loop {
            if self.inflight_count(id) == 0 {
                return;
            }
            if tokio::time::timeout(timeout, self.notify.notified()).await.is_err() {
                return;
            }
        }
    }
}

async fn acquire_owned(
    sem: &Arc<Semaphore>,
    deadline: Option<Duration>,
) -> Result<OwnedSemaphorePermit, KernelError> {
    match deadline {
        Some(d) => match tokio::time::timeout(d, sem.clone().acquire_owned()).await {
            Ok(Ok(p)) => Ok(p),
            Ok(Err(_)) => Err(KernelError::Internal("semaphore closed".into())),
            Err(_) => Err(KernelError::DeadlineExceeded(PluginId::new("<scheduler>"))),
        },
        None => sem
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| KernelError::Internal("semaphore closed".into())),
    }
}
