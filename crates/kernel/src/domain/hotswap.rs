//! 热替换协调器（04 F / 06 §4）。A4：事务开始时快照 `from_gen`，
//! 后续所有回滚/切换都以该快照为准，**禁止在事务内重新查询 `generation(id)`**。

use agent_kernel_core::{Generation, KernelError, PluginId};
use crate::domain::registry::Registry;

pub struct HotSwapPlan {
    pub plugin: PluginId,
    /// 事务开始时的世代快照（回滚 expected 必须填它）。
    pub from_gen: Generation,
    /// 本次计划引入的世代。
    pub to_gen: Generation,
}

pub struct HotSwapCoordinator;

impl HotSwapCoordinator {
    pub fn new() -> Self {
        Self
    }

    /// 规划一次热替换。捕获 from_gen，后续 compare_switch 的 expected 必须填它。
    pub fn plan(&self, registry: &Registry, id: &PluginId) -> Result<HotSwapPlan, KernelError> {
        let from = registry
            .generation_of(id)
            .ok_or_else(|| KernelError::UnknownPlugin(id.clone()))?;
        Ok(HotSwapPlan {
            plugin: id.clone(),
            from_gen: from,
            to_gen: from.next(),
        })
    }
}
