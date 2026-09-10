//! 数据流转（编排）：load / unload / hot-swap 用例（04 / 06）。
//! 这些是 `KernelInner` 的固有方法，集中实现跨层编排，domain 层不反向依赖它们。

use crate::domain::registry::Slot;
use crate::interfaces::domains::domain_for;
use crate::runtime::KernelInner;
use agent_kernel_core::{Generation, KernelError, Manifest, PluginId, PluginState};
use agent_kernel_sdk::{KernelContext, PluginConfig, PluginContext, PluginInstance};
use std::sync::Arc;

impl KernelInner {
    /// 注册一个插件实例：校验 → 依赖解析 → 建域 → init → 注册 → Running。
    pub async fn register_instance(&self, instance: PluginInstance) -> Result<(), KernelError> {
        let manifest = instance.manifest().clone();
        manifest.validate()?; // A3

        // 同 id 已存在则要求 ApiVersion major 一致（K301 规则穿透防护）
        if let Some(existing) = self.registry.slot(&manifest.name) {
            if existing.manifest.api_version.major != manifest.api_version.major {
                return Err(KernelError::ApiVersionMismatch {
                    plugin: manifest.name.clone(),
                    required: existing.manifest.api_version,
                    found: manifest.api_version,
                });
            }
        }

        // 全局依赖解析（含新插件），硬依赖缺失 ⇒ K302，软依赖缺失 ⇒ 补挂跳过（B1）
        let mut manifests: Vec<Manifest> = self
            .registry
            .ids()
            .iter()
            .filter_map(|id| self.registry.slot(id).map(|s| s.manifest.clone()))
            .collect();
        manifests.push(manifest.clone());
        let graph = crate::domain::dependency::resolve(&manifests)?;
        // K2（v0.1.4）：同步刷新 capability → 提供者索引（与依赖图同源，先注册者胜）
        self.cap_index.store(Arc::new(graph.cap_index().clone()));
        *self.deps.lock().unwrap() = graph;

        let domain = domain_for(&manifest, self.scheduler.clone())?;
        let rx = self.lifecycle.new_plugin(&manifest.name);
        let host = self.host().clone();
        let ctx = PluginContext::new(
            manifest.clone(),
            PluginConfig::default(),
            rx,
            KernelContext::new(host, self.config.clone()),
        );
        instance.init(&ctx).await?;

        let slot = Slot {
            plugin: instance,
            generation: Generation::ZERO,
            manifest: manifest.clone(),
        };
        self.registry.register(slot, domain);
        self.lifecycle
            .transition(&manifest.name, PluginState::Running, None)?;
        {
            let mut subs = self.subscriptions.lock().unwrap();
            for ty in &manifest.subscriptions {
                subs.entry(ty.clone()).or_default().push(manifest.name.clone());
            }
        }
        Ok(())
    }

    /// 卸载：Draining → 等待在途 drain（B4）→ destroy → 移除 → Unloaded。
    pub async fn unload(&self, id: &PluginId) -> Result<(), KernelError> {
        self.lifecycle
            .transition(id, PluginState::Draining, None)?;
        self.scheduler
            .drain_wait(id, std::time::Duration::from_millis(5000))
            .await;
        if let Some(slot) = self.registry.slot(id) {
            let _ = slot.plugin.destroy();
        }
        self.registry.remove(id);
        // K2（v0.1.4）：卸载后重建 capability 索引（其余提供者不受影响）
        let remaining: Vec<Manifest> = self
            .registry
            .ids()
            .iter()
            .filter_map(|id| self.registry.slot(id).map(|s| s.manifest.clone()))
            .collect();
        self.cap_index
            .store(Arc::new(crate::domain::dependency::build_cap_index(&remaining)));
        self.lifecycle
            .transition(id, PluginState::Unloaded, None)?;
        self.subscriptions
            .lock()
            .unwrap()
            .retain(|_, v| {
                v.retain(|x| x != id);
                v.is_empty()
            });
        Ok(())
    }

    /// 热替换（不停机）：规划 → 并行 init 新实例 → 迁移 → CAS 切换 → 卸旧。
    pub async fn hot_swap(
        &self,
        id: &PluginId,
        new_instance: PluginInstance,
    ) -> Result<(), KernelError> {
        let new_manifest = new_instance.manifest().clone();
        new_manifest.validate()?;

        // A4：事务开始即快照 from_gen；之后禁止重新查询 generation(id)
        let plan = self.hotswap.plan(&self.registry, id)?;
        let old = self.registry.slot(id);

        // 新实例 init（旧实例继续服务）
        let rx = self
            .lifecycle
            .receiver(id)
            .unwrap_or_else(|| self.lifecycle.new_plugin(id));
        let host = self.host().clone();
        let ctx = PluginContext::new(
            new_manifest.clone(),
            PluginConfig::default(),
            rx,
            KernelContext::new(host, self.config.clone()),
        );
        new_instance.init(&ctx).await?;

        // 迁移：仅当双方都实现 Migratable（C2 默认不支持 ⇒ 丢弃状态）
        if let (Some(old_m), Some(new_m)) = (
            old.as_ref().and_then(|s| s.plugin.as_migratable()),
            new_instance.as_migratable(),
        ) {
            if old_m.is_migratable() && new_m.is_migratable() {
                let snap = old_m.snapshot().await?;
                new_m.restore(&snap).await?;
            }
        }

        // B4（v0.1.3 K1）：CAS 前等待在途请求收敛——与 unload 的 drain 语义对齐。
        // 否则在途调用跨世代执行：旧实例私有的会话控制态（如取消令牌）与新实例断裂。
        // 超时按 unload 同款语义（5s）强制继续，护栏而非无限等待。
        self.scheduler
            .drain_wait(id, std::time::Duration::from_millis(5000))
            .await;

        // CAS 切换（expected = plan.from_gen）
        let new_slot = Slot {
            plugin: new_instance,
            generation: plan.from_gen,
            manifest: new_manifest,
        };
        let _ng = self
            .registry
            .compare_switch(id, plan.from_gen, new_slot)?;

        // 卸下旧实例
        if let Some(old) = old {
            let _ = old.plugin.destroy();
        }
        self.lifecycle
            .transition(id, PluginState::Running, Some("hot-swapped".into()))?;
        Ok(())
    }

    /// B2：panic/cancel 后唯一合法动作 = Failed + 卸载（绝不继续服务）。
    pub async fn handle_plugin_failure(&self, id: &PluginId) {
        let _ = self
            .lifecycle
            .transition(id, PluginState::Failed, Some("panic/cancel -> unload".into()));
        let _ = self.unload(id).await;
    }
}
