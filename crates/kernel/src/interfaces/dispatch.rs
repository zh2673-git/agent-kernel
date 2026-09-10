//! `dispatch`：内核对单次调用的统一入口。落实 B5（快照）/ B2（panic 隔离）/
//! B4（drain 退化为 reject）/ deadline。
//!
//! 关键不变量：
//! - **B5**：进入即一次性快照 `(handle, generation)`，全程不再二次解析。
//! - **B2**：用 `tokio::spawn` 隔离 panic（JoinError::is_panic），捕获后**唯一合法动作**
//!   是 `transition(Failed)` + 卸载，**严禁**仅 `return Err` 后继续服务。
//! - **B4**：Draining 状态直接 `K403` 拒绝，不阻塞等待（避免循环等待）。

use crate::runtime::KernelInner;
use agent_kernel_core::{Envelope, KernelError, PluginId, PluginState};
use serde_json::Value;
use std::sync::Arc;
use tokio::task::JoinError;

pub async fn dispatch(inner: Arc<KernelInner>, env: Envelope) -> Result<Value, KernelError> {
    // B5：一次性快照
    let slot = inner
        .registry
        .slot(&env.target)
        .ok_or_else(|| KernelError::UnknownPlugin(env.target.clone()))?;
    let domain = inner
        .registry
        .domain(&env.target)
        .ok_or_else(|| KernelError::UnknownPlugin(env.target.clone()))?;

    let state = inner.lifecycle.current_state(&env.target);
    if state == PluginState::Draining {
        return Err(KernelError::DrainingReject(env.target.clone()));
    }
    if !state.accepts_new_work() {
        return Err(KernelError::NotRunning(env.target.clone(), state.to_string()));
    }

    // 背压 / per-plugin 在途（B4）；优先级泳道（T8，v0.1.7）：System 走全量闸，
    // Normal 走预留后闸——Normal 打满时 System 仍即时受理。
    let deadline = env.deadline;
    let sched = inner.scheduler.clone(); // acquire 取 self: Arc<Self>，需 owned Arc
    let guards = sched
        .acquire(&env.target, &slot.manifest, state, env.priority, deadline)
        .await?;

    let plugin = slot.plugin.clone();
    let target = env.target.clone();
    let mut handle = tokio::spawn(async move {
        let _g = guards; // 守卫随任务结束 drop ⇒ 在途计数递减（B4 可见）
        domain.dispatch(&plugin, env).await
    });

    // deadline：用 select 竞争，超时则 abort（async 第三类失败，B2'）
    let result = if let Some(d) = deadline {
        let slp = tokio::time::sleep(d);
        tokio::pin!(slp);
        tokio::select! {
            joined = &mut handle => map_join(joined, &target),
            _ = &mut slp => {
                handle.abort();
                Err(KernelError::DeadlineExceeded(target.clone()))
            }
        }
    } else {
        map_join(handle.await, &target)
    };

    // B2：panic ⇒ Failed + 卸载（不继续服务）
    if let Err(KernelError::PluginPanic(ref id, _)) = &result {
        inner.handle_plugin_failure(id).await;
    }
    result
}

fn map_join(join: Result<Result<Value, KernelError>, JoinError>, id: &PluginId) -> Result<Value, KernelError> {
    match join {
        Ok(r) => r,
        Err(e) if e.is_panic() => Err(KernelError::PluginPanic(id.clone(), panic_msg(e))),
        Err(_) => Err(KernelError::Cancelled(id.clone())),
    }
}

fn panic_msg(e: JoinError) -> String {
    if let Ok(payload) = e.try_into_panic() {
        if let Some(s) = payload.downcast_ref::<String>() {
            return s.clone();
        }
        if let Some(s) = payload.downcast_ref::<&str>() {
            return s.to_string();
        }
    }
    "plugin task failed".into()
}
