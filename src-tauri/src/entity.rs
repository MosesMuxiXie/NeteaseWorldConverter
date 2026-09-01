// entity.rs — 对应 EntityPreserver.java：实体/POI/玩家文件的升级迁移与降级保留。

use crate::error::Result;
use crate::sink::Sink;
use crate::version::parse_version;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const KINDS: [&str; 2] = ["entities", "poi"];
const EXTRA_DIRS: [&str; 3] = ["playerdata", "advancements", "stats"];
const LEGACY_DIMENSION_ROOTS: [&str; 3] = ["world", "DIM-1", "DIM1"];

/// 收集输入世界中所有实体/POI 目录（任意命名空间、任意维度，含数据驱动维度），
/// 返回以源路径去重后的列表。新式 `dimensions/<ns>/<dim>/<kind>` 与
/// 旧式 `world|DIM-1|DIM1/<kind>` 都会覆盖——保证"绝不静默丢弃"覆盖到模组数据。
fn collect_entity_dirs(world: &Path) -> Vec<PathBuf> {
    let mut found: BTreeSet<PathBuf> = BTreeSet::new();
    if let Ok(namespaces) = fs::read_dir(world.join("dimensions")) {
        for namespace in namespaces.filter_map(|entry| entry.ok()) {
            let namespace_path = namespace.path();
            if !namespace_path.is_dir() {
                continue;
            }
            if let Ok(dimensions) = fs::read_dir(&namespace_path) {
                for dimension in dimensions.filter_map(|entry| entry.ok()) {
                    let dimension_path = dimension.path();
                    if !dimension_path.is_dir() {
                        continue;
                    }
                    for kind in KINDS {
                        let dir = dimension_path.join(kind);
                        if dir.is_dir() {
                            found.insert(dir);
                        }
                    }
                }
            }
        }
    }
    for legacy_root in LEGACY_DIMENSION_ROOTS {
        for kind in KINDS {
            let dir = world.join(legacy_root).join(kind);
            if dir.is_dir() {
                found.insert(dir);
            }
        }
    }
    found.into_iter().collect()
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
            .map_err(|_| crate::error::ConversionError::from("实体保留内部路径错误"))?;
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

    let entity_dirs = collect_entity_dirs(input_world);
    if downgrade {
        let mut preserved = false;
        for source in &entity_dirs {
            let relative = source
                .strip_prefix(input_world)
                .map_err(|_| crate::error::ConversionError::from("实体保留内部路径错误"))?;
            let destination = output_world.join("_NWC_preserved_source").join(relative);
            copy_dir_tree(source, &destination, sink)?;
            preserved = true;
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
            let readme = output_world
                .join("_NWC_preserved_source")
                .join("README.txt");
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
        // 升级（源 ≤ 目标）：用源文件覆盖输出的实体/POI 目录（保持同相对路径，
        // 因此任意命名空间/维度都能落回原位），并清理输出中的旧式布局残留
        for source in &entity_dirs {
            let relative = source
                .strip_prefix(input_world)
                .map_err(|_| crate::error::ConversionError::from("实体保留内部路径错误"))?;
            let destination = output_world.join(relative);
            let _ = fs::remove_dir_all(&destination);
            copy_dir_tree(source, &destination, sink)?;
        }
        for legacy_root in LEGACY_DIMENSION_ROOTS {
            for kind in KINDS {
                let _ = fs::remove_dir_all(output_world.join(legacy_root).join(kind));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::AppLog;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    fn test_sink(dir: &Path) -> Sink {
        let log = Arc::new(AppLog::new(&dir.join("test.log")).unwrap());
        Sink::new(
            "test".into(),
            Arc::new(AtomicBool::new(false)),
            log,
            |_payload| {},
        )
    }

    #[test]
    fn preserve_covers_all_namespaces_on_downgrade() {
        let input = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        for relative in [
            "dimensions/minecraft/overworld/entities",
            "dimensions/mynamespace/custom_dim/entities",
            "dimensions/minecraft/the_end/poi",
            "world/entities",
        ] {
            let dir = input.path().join(relative);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("r.0.0.mca"), b"entity-data").unwrap();
        }
        let sink = test_sink(output.path());
        let preserved = preserve(input.path(), output.path(), "1.21.10", "1.20.6", &sink).unwrap();
        assert!(preserved);
        let preserved_root = output.path().join("_NWC_preserved_source");
        for relative in [
            "dimensions/minecraft/overworld/entities/r.0.0.mca",
            "dimensions/mynamespace/custom_dim/entities/r.0.0.mca",
            "dimensions/minecraft/the_end/poi/r.0.0.mca",
            "world/entities/r.0.0.mca",
        ] {
            assert!(
                preserved_root.join(relative).exists(),
                "缺少保留数据：{relative}"
            );
        }
    }

    #[test]
    fn upgrade_preserves_custom_namespace_in_place() {
        let input = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let custom = input
            .path()
            .join("dimensions/mynamespace/custom_dim/entities");
        fs::create_dir_all(&custom).unwrap();
        fs::write(custom.join("r.0.0.mca"), b"entity-data").unwrap();
        let sink = test_sink(output.path());
        let preserved = preserve(input.path(), output.path(), "1.20.6", "1.21.10", &sink).unwrap();
        assert!(!preserved);
        // 升级路径按原相对路径落位，不丢失自定义命名空间
        let copied = output
            .path()
            .join("dimensions/mynamespace/custom_dim/entities/r.0.0.mca");
        assert!(copied.exists());
        assert_eq!(fs::read(&copied).unwrap(), b"entity-data");
    }
}
