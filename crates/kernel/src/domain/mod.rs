//! 数据规范落地的核心规则层（domain）：registry / lifecycle / dependency / scheduler / hotswap。
//! 依赖方向：domain 可被 interfaces / application / infrastructure / runtime 引用，但**不**反向依赖它们。

pub mod dependency;
pub mod hotswap;
pub mod lifecycle;
pub mod registry;
pub mod scheduler;

pub use dependency::{resolve, DependencyGraph};
pub use hotswap::{HotSwapCoordinator, HotSwapPlan};
pub use lifecycle::LifecycleManager;
pub use registry::{Registry, Slot};
pub use scheduler::{Guards, Scheduler, SchedulerRef};
