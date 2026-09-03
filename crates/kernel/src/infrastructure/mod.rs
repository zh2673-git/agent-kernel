//! 数据存储/通信底座（infrastructure）。最底层，仅依赖 core/sdk 与 domain 类型。

pub mod bus;

pub use bus::EventBus;
