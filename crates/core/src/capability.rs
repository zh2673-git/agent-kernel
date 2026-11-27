use crate::error::{KernelError, KernelResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 能力声明。插件在清单中声明，内核据此做能力门控（规则契约）。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Capability(pub String);

impl Capability {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 能力集合。用于依赖解析时检查"被依赖方是否声明了所需能力"。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet(pub BTreeSet<Capability>);

impl CapabilitySet {
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }
    pub fn insert(&mut self, c: Capability) {
        self.0.insert(c);
    }
    pub fn contains(&self, c: &Capability) -> bool {
        self.0.contains(c)
    }
    pub fn is_subset_of(&self, other: &CapabilitySet) -> bool {
        self.0.is_subset(&other.0)
    }
    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.0.iter()
    }
}

/// 能力门控（04 §E / ADR-011）：**deny-by-default**——调用方清单未声明的 capability
/// 一律拒绝。授权集由内核从调用方清单的 `dependencies` 构造；本类型只做纯规则判定，
/// 不持有状态，装载期与调用期复用同一判定。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapabilityGate {
    granted: BTreeSet<Capability>,
}

impl CapabilityGate {
    pub fn new() -> Self {
        Self::default()
    }
    /// 由调用方声明的依赖能力构造授权集。
    pub fn from_granted<I: IntoIterator<Item = Capability>>(granted: I) -> Self {
        Self {
            granted: granted.into_iter().collect(),
        }
    }
    pub fn grant(&mut self, c: Capability) {
        self.granted.insert(c);
    }
    pub fn granted(&self) -> impl Iterator<Item = &Capability> {
        self.granted.iter()
    }
    /// 调用期判定：未声明 ⇒ `K405`。
    pub fn check(&self, caller: &crate::PluginId, required: &Capability) -> KernelResult<()> {
        if self.granted.contains(required) {
            Ok(())
        } else {
            Err(KernelError::CapabilityDenied(caller.clone(), required.0.clone()))
        }
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    #[test]
    fn empty_gate_denies_everything() {
        let g = CapabilityGate::new();
        let e = g.check(&crate::PluginId::new("a"), &Capability::new("fs.write")).unwrap_err();
        assert_eq!(e.code(), "K405");
    }

    #[test]
    fn declared_capability_passes_and_undeclared_is_denied() {
        let g = CapabilityGate::from_granted([Capability::new("svc.echo")]);
        assert!(g.check(&crate::PluginId::new("a"), &Capability::new("svc.echo")).is_ok());
        assert_eq!(
            g.check(&crate::PluginId::new("a"), &Capability::new("svc.rm")).unwrap_err().code(),
            "K405"
        );
    }
}
