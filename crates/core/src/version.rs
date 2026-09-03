use crate::KernelError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// 插件自身版本（语义化）。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for Version {
    type Err = KernelError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut it = s.trim().split('.');
        let major = it
            .next()
            .and_then(|x| x.parse().ok())
            .ok_or_else(|| KernelError::ManifestParse(format!("bad version: {s}")))?;
        let minor = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let patch = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        Ok(Version::new(major, minor, patch))
    }
}

/// 契约（ApiVersion）版本。仅 major.minor，用于加载期规则校验（03 §2.2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
}

impl ApiVersion {
    pub fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
    /// 兼容性判定：major 必须相等，minor 必须 >= 要求（向后兼容，允许新增可选能力）。
    pub fn is_compatible_with(&self, required: &ApiVersion) -> bool {
        self.major == required.major && self.minor >= required.minor
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}", self.major, self.minor)
    }
}

impl FromStr for ApiVersion {
    type Err = KernelError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().trim_start_matches('v');
        let mut it = s.split('.');
        let major = it
            .next()
            .and_then(|x| x.parse().ok())
            .ok_or_else(|| KernelError::ManifestParse(format!("bad api version: {s}")))?;
        let minor = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        Ok(ApiVersion::new(major, minor))
    }
}
