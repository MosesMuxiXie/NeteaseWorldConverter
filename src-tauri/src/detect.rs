// 对应 WorldDetector.java：识别 Java / 基岩 / 网易加密基岩存档。

use crate::archive::{file_name, is_ignored_path};
use crate::error::Result;
use crate::log::AppLog;
use crate::models::{WorldInfo, WorldType};
use crate::nbt;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const NETEASE_HEADER: [u8; 4] = [0x80, 0x1D, 0x30, 0x01];
pub const LEGACY_AES_HEADER: [u8; 4] = [0x90, 0x1D, 0x30, 0x01];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Java,
    Bedrock,
}

#[derive(Clone)]
struct Candidate {
    root: PathBuf,
    database_directory: Option<PathBuf>,
    kind: Kind,
    score: i64,
}

/// 与 Java 正则 `db(?:[ _-]*\d+)?` 全串匹配语义一致。
pub fn matches_db_directory(name: &str) -> bool {
    if name == "db" {
        return true;
    }
    if !name.starts_with("db") {
        return false;
    }
    let rest = &name[2..];
    match rest.find(|c| c != ' ' && c != '_' && c != '-') {
        Some(index) => {
            let digits = &rest[index..];
            !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

pub fn starts_with_header(path: &Path, expected: &[u8]) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut buffer = [0u8; 4];
    let mut filled = 0;
    while filled < expected.len() {
        match file.read(&mut buffer[filled..expected.len()]) {
            Ok(0) => return false,
            Ok(n) => filled += n,
            Err(_) => return false,
        }
    }
    buffer[..expected.len()] == *expected
}

fn contains_mca(root: &Path, depth: usize) -> bool {
    WalkDir::new(root)
        .max_depth(depth)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            entry.file_type().is_file() && file_name(entry.path()).to_lowercase().ends_with(".mca")
        })
}

fn contains_leveldb_files(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    entries.filter_map(|entry| entry.ok()).any(|entry| {
        if !entry.path().is_file() {
            return false;
        }
        let name = file_name(&entry.path()).to_uppercase();
        name.ends_with(".LDB")
            || name.ends_with(".LOG")
            || name.starts_with("MANIFEST-")
            || name == "CURRENT"
    })
}

fn has_header(candidate: &Candidate, expected: &[u8]) -> bool {
    let mut files: Vec<PathBuf> = Vec::new();
    if let Some(db) = &candidate.database_directory {
        if let Ok(entries) = fs::read_dir(db) {
            for entry in entries.filter_map(|entry| entry.ok()) {
                if entry.path().is_file() {
                    files.push(entry.path());
                }
            }
        }
    }
    if let Ok(entries) = fs::read_dir(&candidate.root) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if path.is_file() && file_name(&path).starts_with("MANIFEST-") {
                files.push(path);
            }
        }
    }
    files.iter().any(|file| starts_with_header(file, expected))
}

fn contains_netease_marker(root: &Path) -> bool {
    WalkDir::new(root)
        .max_depth(2)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .any(|entry| file_name(entry.path()).to_lowercase().contains("netease"))
}

pub fn detect(extracted: &Path, log: &AppLog) -> Result<WorldInfo> {
    let mut directories: Vec<PathBuf> = WalkDir::new(extracted)
        .max_depth(10)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_dir())
        .map(|entry| entry.into_path())
        .filter(|path| !is_ignored_path(path, extracted))
        .collect();
    directories.sort_by_key(|path| path.components().count());

    let mut candidates: Vec<Candidate> = Vec::new();
    for directory in &directories {
        if directory.join("level.dat").is_file() {
            let mut score: i64 = 20;
            if contains_mca(directory, 5) {
                score += 50;
            }
            if directory.join("region").is_dir() || directory.join("dimensions").is_dir() {
                score += 15;
            }
            score -= depth(extracted, directory) as i64;
            candidates.push(Candidate {
                root: directory.clone(),
                database_directory: None,
                kind: Kind::Java,
                score,
            });
        }

        let name = file_name(directory).to_lowercase();
        if matches_db_directory(&name) && contains_leveldb_files(directory) {
            if let Some(world_root) = directory.parent() {
                if world_root.starts_with(extracted) {
                    let score = 100 - depth(extracted, world_root) as i64;
                    candidates.push(Candidate {
                        root: world_root.to_path_buf(),
                        database_directory: Some(directory.clone()),
                        kind: Kind::Bedrock,
                        score,
                    });
                }
            }
        }
    }

    let encrypted_bedrock = candidates
        .iter()
        .filter(|candidate| candidate.kind == Kind::Bedrock)
        .filter(|candidate| {
            has_header(candidate, &NETEASE_HEADER) || has_header(candidate, &LEGACY_AES_HEADER)
        })
        .max_by_key(|candidate| candidate.score)
        .cloned();
    let selected = match encrypted_bedrock {
        Some(candidate) => candidate,
        None => candidates
            .iter()
            .max_by_key(|candidate| candidate.score)
            .cloned()
            .ok_or_else(|| {
                crate::error::ConversionError::from(
                    "没有找到可识别的 Minecraft 世界根目录。需要 Java level.dat/region，或基岩版 db/*.ldb。",
                )
            })?,
    };

    let mut file_count: u64 = 0;
    let mut byte_count: u64 = 0;
    for entry in WalkDir::new(&selected.root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
    {
        if is_ignored_path(&entry, &selected.root) {
            continue;
        }
        if let Ok(metadata) = fs::metadata(&entry) {
            file_count += 1;
            byte_count += metadata.len();
        }
    }

    let mut notes: Vec<String> = Vec::new();
    let (world_type, version, world_name);
    if selected.kind == Kind::Java {
        world_type = WorldType::Java;
        let metadata = nbt::read_java_level(&selected.root.join("level.dat")).map_err(|error| {
            crate::error::ConversionError::from(format!(
                "找到了 Java 目录，但 level.dat 无法解析；{error}"
            ))
        })?;
        version = if metadata.version_name.is_empty() {
            format!("DataVersion {}", metadata.data_version)
        } else {
            metadata.version_name.clone()
        };
        world_name = if metadata.world_name.is_empty() {
            file_name(&selected.root)
        } else {
            metadata.world_name.clone()
        };
        if contains_netease_marker(&selected.root) {
            notes.push("发现网易标记文件，按网易 Java/标准 Java Anvil 存档处理".into());
        } else {
            notes.push("Java 存档本身没有统一的网易专用签名，按标准 Anvil 格式处理".into());
        }
    } else {
        let modern_encrypted = has_header(&selected, &NETEASE_HEADER);
        let legacy_encrypted = has_header(&selected, &LEGACY_AES_HEADER);
        world_type = if legacy_encrypted {
            notes.push("检测到 90 1D 30 01 旧版 AES-CFB8 加密头；该格式无法离线恢复密钥".into());
            WorldType::NeteaseBedrockLegacyAes
        } else if modern_encrypted {
            notes.push("检测到 80 1D 30 01 网易异或加密头".into());
            WorldType::NeteaseBedrock
        } else {
            notes.push("LevelDB 未加密，将直接规范化目录".into());
            WorldType::Bedrock
        };
        let root_level = selected.root.join("level.dat");
        let nested_level = selected
            .database_directory
            .as_ref()
            .map(|db| db.join("level.dat"));
        if !root_level.is_file() && nested_level.is_some_and(|path| path.is_file()) {
            notes.push("level.dat 位于数据库目录内，转换时会自动移回世界根目录".into());
        }
        version = "基岩 LevelDB".to_string();
        world_name = file_name(&selected.root);
    }

    log.info(&format!(
        "识别结果：{}，根目录={}",
        world_type.display_name(),
        selected.root.display()
    ));
    log.info(&format!("存档文件：{file_count} 个，{byte_count} 字节"));
    for note in &notes {
        log.info(note);
    }
    Ok(WorldInfo {
        root: selected.root,
        database_directory: selected.database_directory,
        world_type,
        detected_version: version,
        world_name,
        file_count,
        byte_count,
        notes,
    })
}

fn depth(root: &Path, child: &Path) -> usize {
    child
        .strip_prefix(root)
        .map(|p| p.components().count())
        .unwrap_or(0)
}

// 供 decrypt.rs 复用的目录收集工具
pub fn list_regular_files(directory: &Path) -> BTreeMap<String, PathBuf> {
    let mut files = BTreeMap::new();
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if path.is_file() {
                files.insert(file_name(&path), path);
            }
        }
    }
    files
}
