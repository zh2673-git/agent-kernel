//! 事件总线（时间流的物理载体）。
//!
//! 分层（B1）：
//! - **事件类（审计/指标）**走 `broadcast`（多消费者、可丢）。
//! - **状态类（生命周期）**走 `watch`（每插件一个，订阅即得当前值、可重放，无 Lagged 静默丢状态）。
//! - 主事件通道为**有界 mpsc**（背压的物理前提，02 选型）。

use agent_kernel_core::{Event, LifecycleEvent};
use tokio::sync::{broadcast, mpsc};

pub struct EventBus {
    event_tx: mpsc::Sender<Event>,
    event_rx: tokio::sync::Mutex<Option<mpsc::Receiver<Event>>>,
    lifecycle_tx: broadcast::Sender<LifecycleEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel::<Event>(1024); // 有界 ⇒ 背压
        let (lifecycle_tx, _rx) = broadcast::channel::<LifecycleEvent>(256);
        Self {
            event_tx,
            event_rx: tokio::sync::Mutex::new(Some(event_rx)),
            lifecycle_tx,
        }
    }

    /// 主循环从此取接收端（仅可取一次）。
    pub fn take_event_rx(&self) -> mpsc::Receiver<Event> {
        self.event_rx
            .try_lock()
            .expect("take_event_rx called once")
            .take()
            .expect("event_rx already taken")
    }

    /// 发布事件（await ⇒ 背压生效）。
    pub async fn publish_event(&self, e: Event) -> Result<(), agent_kernel_core::KernelError> {
        self.event_tx
            .send(e)
            .await
            .map_err(|_| agent_kernel_core::KernelError::Internal("event bus closed".into()))
    }

    pub fn publish_lifecycle(&self, le: LifecycleEvent) {
        let _ = self.lifecycle_tx.send(le);
    }

    pub fn subscribe_lifecycle(&self) -> broadcast::Receiver<LifecycleEvent> {
        self.lifecycle_tx.subscribe()
    }

    /// 生命周期 broadcast 发送端（供 LifecycleManager 使用）。
    pub fn lifecycle_tx(&self) -> broadcast::Sender<LifecycleEvent> {
        self.lifecycle_tx.clone()
    }
}
