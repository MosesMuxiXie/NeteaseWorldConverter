// entity.rs — 对应 EntityPreserver.java：实体/POI/玩家文件的升级迁移与降级保留。

use crate::error::Result;
use crate::sink::Sink;
use crate::version::parse_version;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const DIMENSIONS: [&str; 3] = ["overworld", "the_nether", "the_end"];
const KINDS: [&str; 2] = ["entities", "poi"];
const EXTRA_DIRS: [&str; 3] = ["playerdata", "advancements", "stats"];

fn legacy_dimension(dim: &str) -> Option<&'static str> {
    match dim {
        "overworld" => Some("world"),
        "the_nether" => Some("DIM-1"),
        "the_end" => Some("DIM1"),
        _ => None,
    }
}

/// 在输入世界内定位某维度某类文件（新式 dimensions/... 优先，旧式 world/DIM-1/DIM1 回退）。
fn find_dir(world: &Path, dim: &str, kind: &str) -> Option<PathBuf> {
    let modern = world.join("dimensions/minecraft").join(dim).join(kind);
    if modern.is_dir() {
        return Some(modern);
    }
    let legacy = world.join(legacy_dimension(dim)?).join(kind);
    if legacy.is_dir() {
        Some(legacy)
    } else {
        None
    }
}

fn copy_dir_tree(source: &Path, destination: &Path, sink: &Sink) -> Result<()> {
    let mut files: Vec<PathBuf> = WalkDir::new(source)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect();
    files.sort();
    for (index, file) in files.iter().enumerate() {
        sink.check_cancel()?;
        let relative = file
            .strip_prefix(source)
            .map_err(|_| crate::error::ConversionError("实体保留内部路径错误".into()))?;
        let target = destination.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(file, &target)?;
        sink.update(
            84,
            "实体/POI 保留",
            &format!("{} / {} 个文件", index + 1, files.len()),
        );
    }
    Ok(())
}

/// 升级：删除输出对应目录后整树复制（NBT 由目标版本 DataFixer 首次加载时升级）；
/// 降级：全部复制到输出 `_NWC_preserved_source/` 原目录结构下，不静默丢弃。
/// 返回是否写入了 preserved_source（供引擎在报告中备注）。
pub fn preserve(
    input_world: &Path,
    output_world: &Path,
    source_version: &str,
    target_version: &str,
    sink: &Sink,
) -> Result<bool> {
    let downgrade = match (parse_version(source_version), parse_version(target_version)) {
        (Some(source), Some(target)) => source > target,
        // 无法解析版本时按降级保守处理：preserved_source 对 Minecraft 无副作用
        _ => true,
    };

    if downgrade {
        let mut preserved = false;
        for dim in DIMENSIONS {
            for kind in KINDS {
                if let Some(source) = find_dir(input_world, dim, kind) {
                    let relative = source
                        .strip_prefix(input_world)
                        .map_err(|_| crate::error::ConversionError("实体保留内部路径错误".into()))?;
                    let destination = output_world.join("_NWC_preserved_source").join(relative);
                    copy_dir_tree(&source, &destination, sink)?;
                    preserved = true;
                }
            }
        }
        for extra in EXTRA_DIRS {
            let source = input_world.join(extra);
            if source.is_dir() {
                let destination = output_world.join("_NWC_preserved_source").join(extra);
                copy_dir_tree(&source, &destination, sink)?;
                preserved = true;
            }
        }
        if preserved {
            let readme = output_world.join("_NWC_preserved_source").join("README.txt");
            if let Some(parent) = readme.parent() {
                fs::create_dir_all(parent)?;
            }
            let note = format!(
                "这是降级转换（{source_version} → {target_version}）。\n\
本目录保存了目标版本无法表达的源实体、POI 与玩家数据，转换器不会静默丢弃它们。\n\
原 ZIP 始终未修改；如需完整数据请继续使用原存档。\n"
            );
            fs::write(&readme, note)?;
        }
        Ok(preserved)
    } else {
        // 升级（源 ≤ 目标）：用源文件覆盖输出的实体/POI/玩家目录
        for dim in DIMENSIONS {
            for kind in KINDS {
                if let Some(source) = find_dir(input_world, dim, kind) {
                    let destination = output_world.join("dimensions/minecraft").join(dim).join(kind);
                    let _ = fs::remove_dir_all(&destination);
                    copy_dir_tree(&source, &destination, sink)?;
                    if let Some(legacy) = legacy_dimension(dim) {
                        let legacy_destination = output_world.join(legacy).join(kind);
                        let _ = fs::remove_dir_all(&legacy_destination);
                    }
                }
            }
        }
        for extra in EXTRA_DIRS {
            let source = input_world.join(extra);
            if source.is_dir() {
                let destination = output_world.join(extra);
                let _ = fs::remove_dir_all(&destination);
                copy_dir_tree(&source, &destination, sink)?;
            }
        }
        Ok(false)
    }
}

