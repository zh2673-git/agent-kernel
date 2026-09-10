//! 插件依赖图与加载顺序（07）。petgraph 做拓扑排序 + 环检测（02 选型）。
//!
//! 依赖解析规则（07 §6.1）：
//! - 硬依赖（hard）找不到提供方 ⇒ 拒绝加载（K302）。
//! - 软依赖（soft）找不到提供方 ⇒ 补挂跳过（B1：先订阅再读当前值）。

use agent_kernel_core::{Capability, KernelError, Manifest, PluginId};
use petgraph::algo;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

pub struct DependencyGraph {
    graph: DiGraph<PluginId, ()>,
    index: HashMap<PluginId, NodeIndex>,
    /// capability → 提供者（与图同生命周期，resolve 时一并重建；K2 寻址数据源）。
    cap_index: HashMap<Capability, PluginId>,
}

impl DependencyGraph {
    /// 查 capability 的当前提供者（K2）。
    pub fn cap_provider(&self, capability: &Capability) -> Option<&PluginId> {
        self.cap_index.get(capability)
    }
    /// 索引只读视图（K2：KernelInner 注册时同步到 ArcSwap 缓存）。
    pub fn cap_index(&self) -> &HashMap<Capability, PluginId> {
        &self.cap_index
    }
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index: HashMap::new(),
            cap_index: HashMap::new(),
        }
    }
    fn node(&mut self, id: &PluginId) -> NodeIndex {
        if let Some(n) = self.index.get(id) {
            return *n;
        }
        let n = self.graph.add_node(id.clone());
        self.index.insert(id.clone(), n);
        n
    }
    pub fn add_edge(&mut self, from: &PluginId, to: &PluginId) {
        let a = self.node(from);
        let b = self.node(to);
        self.graph.add_edge(a, b, ());
    }
}

/// capability → 提供者索引（K2 寻址契约的数据源）。
/// 规则：**先注册者胜**（`or_insert` 语义）——调用方须按「注册表现有槽位 + 新插件」
/// 的顺序传入。多 provider 路由策略（负载均衡/优先级）等真实需求出现再议（见 PLAN 已评估不做）。
pub fn build_cap_index(manifests: &[Manifest]) -> HashMap<Capability, PluginId> {
    let mut cap_index: HashMap<Capability, PluginId> = HashMap::new();
    for m in manifests {
        for c in &m.capabilities {
            cap_index.entry(c.clone()).or_insert_with(|| m.name.clone());
        }
    }
    cap_index
}

/// 解析全部清单，构建依赖图并校验（硬依赖 / 环检测）。
pub fn resolve(manifests: &[Manifest]) -> Result<DependencyGraph, KernelError> {
    let cap_index = build_cap_index(manifests);

    let mut g = DependencyGraph::new();
    g.cap_index = cap_index;
    for m in manifests {
        g.node(&m.name);
        for dep in &m.dependencies {
            match g.cap_index.get(&dep.capability).cloned() {
                Some(provider) if provider != m.name => {
                    g.add_edge(&m.name, &provider);
                }
                Some(_) => { /* 自提供，无需边 */ }
                None => {
                    if dep.hard {
                        return Err(KernelError::HardDependencyUnsatisfied(
                            m.name.clone(),
                            dep.capability.0.clone(),
                        ));
                    }
                    // 软依赖缺失：补挂跳过（B1）
                }
            }
        }
    }

    if let Err(cycle) = algo::toposort(&g.graph, None) {
        return Err(KernelError::DependencyCycle(format!("{:?}", cycle.node_id())));
    }
    Ok(g)
}
