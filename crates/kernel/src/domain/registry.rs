//! 注册表（空间归属的真相源）。以 `ArcSwap` 整体原子替换，读侧无锁（02 选型）。
//! 每个插件槽位持有 `Arc<dyn Plugin>`，世代随热替换递增（B5 / A4）。

use crate::interfaces::ExecutionDomain;
use agent_kernel_core::{Generation, KernelError, Manifest, PluginId};
use agent_kernel_sdk::PluginInstance;
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::Arc;

/// 单个插件槽位。可克隆（plugin 为 Arc、manifest 为 Clone、generation 为 Copy）。
#[derive(Clone)]
pub struct Slot {
    pub plugin: PluginInstance,
    pub generation: Generation,
    pub manifest: Manifest,
}

pub struct Registry {
    slots: Arc<ArcSwap<HashMap<PluginId, Slot>>>,
    /// 执行域适配器按插件稳定持有（跨世代不变，同插件类型同域）。
    domains: std::sync::Mutex<HashMap<PluginId, Arc<dyn ExecutionDomain>>>,
    /// 写锁：保证 compare_switch 的「比较-交换」区间内独占（读侧仍可无锁）。
    write_lock: std::sync::Mutex<()>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            slots: Arc::new(ArcSwap::new(Arc::new(HashMap::new()))),
            domains: std::sync::Mutex::new(HashMap::new()),
            write_lock: std::sync::Mutex::new(()),
        }
    }

    /// 注册新插件，世代从 ZERO 起算。
    pub fn register(&self, slot: Slot, domain: Arc<dyn ExecutionDomain>) -> Generation {
        let name = slot.manifest.name.clone();
        let mut map = (**self.slots.load()).clone();
        let g = Generation::ZERO;
        map.insert(name.clone(), Slot { generation: g, ..slot });
        self.domains.lock().unwrap().insert(name, domain);
        self.slots.store(Arc::new(map));
        g
    }

    /// B5：一次性快照槽位（含 plugin + generation）。
    pub fn slot(&self, id: &PluginId) -> Option<Slot> {
        self.slots.load().get(id).cloned()
    }

    pub fn domain(&self, id: &PluginId) -> Option<Arc<dyn ExecutionDomain>> {
        self.domains.lock().unwrap().get(id).cloned()
    }

    pub fn generation_of(&self, id: &PluginId) -> Option<Generation> {
        self.slots.load().get(id).map(|s| s.generation)
    }

    /// A4：CAS 世代切换。仅当当前世代 == expected 时替换为 next 世代；
    /// 失败返回 K504（回滚时填错 expected 会立刻暴露，而非潜伏）。
    ///
    /// 实现：读侧无锁（`ArcSwap`）；写侧在 `write_lock` 保护下做「比较-交换」，
    /// 等价于 CAS，且回滚时 `expected` 必须填本次计划引入的世代（A4）。
    pub fn compare_switch(
        &self,
        id: &PluginId,
        expected: Generation,
        mut new_slot: Slot,
    ) -> Result<Generation, KernelError> {
        let _wl = self.write_lock.lock().unwrap();
        let cur = self.slots.load_full();
        match cur.get(id) {
            Some(s) if s.generation == expected => {
                let ng = expected.next();
                let mut m = (*cur).clone();
                new_slot.generation = ng;
                m.insert(id.clone(), new_slot);
                self.slots.store(Arc::new(m));
                Ok(ng)
            }
            Some(s) => Err(KernelError::GenerationMismatch {
                plugin: id.clone(),
                expected,
                found: s.generation,
            }),
            None => Err(KernelError::UnknownPlugin(id.clone())),
        }
    }

    pub fn remove(&self, id: &PluginId) -> Option<Slot> {
        let mut map = (**self.slots.load()).clone();
        let r = map.remove(id);
        self.slots.store(Arc::new(map));
        self.domains.lock().unwrap().remove(id);
        r
    }

    pub fn ids(&self) -> Vec<PluginId> {
        self.slots.load().keys().cloned().collect()
    }
}
