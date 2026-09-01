// version.rs — Java 版本号解析与比较（对应 ConversionEngine.isDowngrade 的三段比较）。

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
}

/// je2be（b2j）输出的固定中间版本：基岩→Java 先落到该版本，再由 Chunker 跨到目标版本。
pub const JE2BE_INTERMEDIATE: Version = Version {
    major: 1,
    minor: 21,
    patch: 10,
};

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.patch > 0 {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        } else {
            write!(f, "{}.{}", self.major, self.minor)
        }
    }
}

static VERSION_RE: OnceLock<Regex> = OnceLock::new();

/// 从字符串中提取 `26.x(.x)?` 或 `1.x(.x)?` 版本号并做数值化解析。
pub fn parse_version(text: &str) -> Option<Version> {
    let re = VERSION_RE.get_or_init(|| Regex::new(r"(?:26|1)\.\d+(?:\.\d+)?").expect("版本正则"));
    let matched = re.find(text)?;
    let parts: Vec<&str> = matched.as_str().split('.').collect();
    let major: i32 = parts.first()?.parse().ok()?;
    let minor: i32 = parts.get(1)?.parse().ok()?;
    let patch: i32 = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0);
    Some(Version {
        major,
        minor,
        patch,
    })
}

/// 降级判断：源 > 目标。任一版本无法解析时返回 None（由调用方决定保守策略）。
pub fn is_downgrade_opt(source: &str, target: &str) -> Option<bool> {
    match (parse_version(source), parse_version(target)) {
        (Some(s), Some(t)) => Some(s > t),
        _ => None,
    }
}

/// Chunker 目标格式（如 JAVA_1_21_10）。
pub fn chunker_format(version: Version) -> String {
    let Version {
        major,
        minor,
        patch,
    } = version;
    if patch > 0 {
        format!("JAVA_{major}_{minor}_{patch}")
    } else {
        format!("JAVA_{major}_{minor}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_java_versions() {
        assert_eq!(
            parse_version("Java 1.21.10"),
            Some(Version {
                major: 1,
                minor: 21,
                patch: 10
            })
        );
        assert_eq!(
            parse_version("26.2"),
            Some(Version {
                major: 26,
                minor: 2,
                patch: 0
            })
        );
        assert_eq!(parse_version("基岩 LevelDB"), None);
    }

    #[test]
    fn compares_versions() {
        assert_eq!(is_downgrade_opt("1.21.10", "Java 1.20.6"), Some(true));
        assert_eq!(is_downgrade_opt("1.16.5", "1.21.4"), Some(false));
        assert_eq!(is_downgrade_opt("1.21.0", "1.21.0"), Some(false));
    }

    #[test]
    fn formats_chunker_targets() {
        assert_eq!(
            chunker_format(Version {
                major: 1,
                minor: 21,
                patch: 10
            }),
            "JAVA_1_21_10"
        );
        assert_eq!(
            chunker_format(Version {
                major: 26,
                minor: 2,
                patch: 0
            }),
            "JAVA_26_2"
        );
    }
}
