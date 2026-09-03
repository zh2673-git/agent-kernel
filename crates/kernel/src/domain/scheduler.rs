//! 调度器（时间编排）：优先级泳道、背压（有界许可）、per-plugin 在途上限、
//! drain 等待（B4）。串行语义由执行域在 dispatch 内加锁实现，此处只管在途许可。

use agent_kernel_core::{KernelError, Manifest, PluginId, PluginState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

pub type SchedulerRef = Arc<Scheduler>;

pub struct Scheduler {
    /// 全局在途上限（背压第一道闸）。
    total: Arc<Semaphore>,
    /// 每插件在途上限（manifest.max_inflight，B4）。
    per_plugin: Mutex<HashMap<PluginId, Arc<Semaphore>>>,
    /// 当前在途计数（drain 判定）。
    inflight: Mutex<HashMap<PluginId, usize>>,
    /// drain 完成时唤醒等待者。
    notify: Arc<Notify>,
}

/// 在途许可守卫。Drop 时递减在途计数并通知（B4 drain 可见性）。
pub struct Guards {
    _total: OwnedSemaphorePermit,
    _per: OwnedSemaphorePermit,
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
        Arc::new(Self {
            total: Arc::new(Semaphore::new(max_total.max(1))),
            per_plugin: Mutex::new(HashMap::new()),
            inflight: Mutex::new(HashMap::new()),
            notify: Arc::new(Notify::new()),
        })
    }

    /// 获取在途许可。Draining ⇒ 退化为 Reject（K403，B4）。
    pub async fn acquire(
        self: Arc<Self>,
        id: &PluginId,
        manifest: &Manifest,
        state: PluginState,
        deadline: Option<Duration>,
    ) -> Result<Guards, KernelError> {
        if state == PluginState::Draining {
            return Err(KernelError::DrainingReject(id.clone()));
        }
        let cap = manifest.max_inflight.unwrap_or(16).max(1);
        let per = self
            .per_plugin
            .lock()
            .unwrap()
            .entry(id.clone())
            .or_insert_with(|| Arc::new(Semaphore::new(cap)))
            .clone();
        let total = acquire_owned(&self.total, deadline).await?;
        let per_guard = acquire_owned(&per, deadline).await?;
        *self.inflight.lock().unwrap().entry(id.clone()).or_insert(0) += 1;
        Ok(Guards {
            _total: total,
            _per: per_guard,
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
