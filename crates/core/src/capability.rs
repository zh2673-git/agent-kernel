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
