use crate::KernelError;
use async_trait::async_trait;

/// 热迁移协议（状态可丢弃 or 可快照迁移，docs/program.md 空间契约）。
///
/// **C2（强制）**：默认 `is_migratable() == false`，`snapshot`/`restore` 默认返回
/// 类型化 `Err(K600)`。**严禁** `unimplemented!()` / `todo!()` / `panic!()` ——
/// 在一个以"插件 panic 不拖垮内核"为核心卖点的项目里，SDK 热路径放 panic 宏是自毁招牌。
#[async_trait]
pub trait Migratable: Send + Sync {
    /// 是否支持热迁移。默认 false（不支持 → 热替换时丢弃状态）。
    fn is_migratable(&self) -> bool {
        false
    }

    /// 导出可序列化快照。默认返回 K600（不支持）。
    async fn snapshot(&self) -> Result<Vec<u8>, KernelError> {
        Err(KernelError::MigrationUnsupported(
            crate::PluginId::new("<unknown>"),
        ))
    }

    /// 从快照恢复。默认返回 K600。
    async fn restore(&self, _data: &[u8]) -> Result<(), KernelError> {
        Err(KernelError::MigrationUnsupported(
            crate::PluginId::new("<unknown>"),
        ))
    }
}
