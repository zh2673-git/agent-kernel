use serde::{Deserialize, Serialize};
use std::fmt;

/// 世代号。热替换时每次成功替换递增；用于 CAS 回滚与"读撕裂"防护（B5）。
///
/// 语义：`Generation` 是一个单调递增计数器。注册表中每个插件槽位持有
/// `ArcSwap<(Arc<dyn Plugin>, Generation)>`；`dispatch` 开始时一次性快照
/// `(handle, generation)`，此后全程不再二次解析——在途请求跑在旧世代上是
/// 正确且期望的。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Generation(pub u64);

impl Generation {
    pub const ZERO: Generation = Generation(0);

    pub fn next(self) -> Generation {
        Generation(self.0 + 1)
    }
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Generation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "gen#{}", self.0)
    }
}
