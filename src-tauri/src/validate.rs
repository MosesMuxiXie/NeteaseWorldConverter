// validate.rs — 对应 JavaWorldValidator.java：对输出 Anvil 世界逐区域做结构验证。

use crate::archive::file_name;
use crate::error::{conv, ConversionError, Result};
use crate::models::ValidationResult;
use crate::nbt;
use crate::sink::Sink;
use flate2::read::{GzDecoder, ZlibDecoder};
use std::collections::BTreeSet;
use std::fs;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn validate(world: &Path, sink: &Sink) -> Result<ValidationResult> {
    let level = nbt::read_java_level(&world.join("level.dat"))
        .map_err(|error| ConversionError(format!("输出 level.dat 无法解析：{error}")))?;

    let mut regions: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(world).into_iter().filter_map(|entry| entry.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(world)
            .map_err(|_| ConversionError("验证内部路径错误".into()))?;
        if relative
            .components()
            .next()
            .is_some_and(|part| part.as_os_str() == "_NWC_preserved_source")
        {
            continue;
        }
        if entry
            .path()
            .extension()
            .is_some_and(|ext| ext.to_string_lossy().to_lowercase() == "mca")
        {
            // 原版存档中常见 0 字节区域文件（Minecraft 预生成的空占位），视为空区域跳过
            if entry.metadata().map(|m| m.len()).unwrap_or(0) == 0 {
                continue;
            }
            regions.push(entry.into_path());
        }
    }
    if regions.is_empty() {
        return conv("输出世界没有任何 .mca 区域文件");
    }
    regions.sort();

    let total = regions.len();
    let mut region_chunks: i64 = 0;
    for (index, region) in regions.iter().enumerate() {
        sink.check_cancel()?;
        let chunks = validate_region(region)?;
        region_chunks += chunks as i64;
        let percent = 85 + (9i64 * (index as i64 + 1) / total as i64) as i32;
        sink.update(
            percent,
            "验证 Anvil 区域",
            &format!("{} / {} 个区域文件", index + 1, total),
        );
    }

    let (files, bytes) = count_output(world)?;
    let level_version = if level.version_name.is_empty() {
        format!("DataVersion {}", level.data_version)
    } else {
        level.version_name
    };
    sink.log.info(&format!(
        "结构验证通过：{total} 个区域文件，{region_chunks} 个 chunk；输出共 {files} 个文件，{bytes} 字节"
    ));
    Ok(ValidationResult {
        region_files: total as i64,
        region_chunks,
        files,
        bytes,
        level_version,
        data_version: level.data_version,
    })
}

/// 校验单个 .mca：大小、位置表、扇区不重叠、记录长度、压缩类型、外部 .mcc、NBT 树。
fn validate_region(path: &Path) -> Result<usize> {
    let name = file_name(path);
    let size = fs::metadata(path)?.len();
    if size < 8192 || size % 4096 != 0 {
        return conv(format!(
            "区域文件 {name} 大小非法（{size} 字节，应为 4096 的倍数且不小于 8192）"
        ));
    }
    let mut file = fs::File::open(path)?;
    let mut header = vec![0u8; 8192];
    file.read_exact(&mut header)?;

    let mut used_sectors: BTreeSet<u32> = BTreeSet::new();
    used_sectors.insert(0);
    used_sectors.insert(1);
    let mut chunks = 0usize;

    for index in 0..1024 {
        let base = index * 4;
        let offset = ((header[base] as u32) << 16)
            | ((header[base + 1] as u32) << 8)
            | (header[base + 2] as u32);
        let sectors = header[base + 3] as u32;
        if offset == 0 && sectors == 0 {
            continue;
        }
        if offset < 2 || sectors == 0 {
            return conv(format!(
                "区域文件 {name} 位置表第 {index} 项非法（offset={offset}，sectors={sectors}）"
            ));
        }
        let start = (offset as u64) * 4096;
        let end = ((offset + sectors) as u64) * 4096;
        if end > size {
            return conv(format!("区域文件 {name} 第 {index} 项数据越界"));
        }
        for sector in offset..offset + sectors {
            if !used_sectors.insert(sector) {
                return conv(format!(
                    "区域文件 {name} 第 {index} 项扇区重叠（扇区 {sector} 被重复使用）"
                ));
            }
        }

        file.seek(SeekFrom::Start(start))?;
        let mut length_bytes = [0u8; 4];
        file.read_exact(&mut length_bytes)?;
        let length = i32::from_be_bytes(length_bytes);
        if length < 1 {
            return conv(format!(
                "区域文件 {name} 第 {index} 项记录长度非法（{length}）"
            ));
        }
        if (length as u64) + 4 > sectors as u64 * 4096 {
            return conv(format!(
                "区域文件 {name} 第 {index} 项记录长度超过其扇区容量"
            ));
        }

        let mut compression_byte = [0u8; 1];
        file.read_exact(&mut compression_byte)?;
        let external = compression_byte[0] & 0x80 != 0;
        let compression = compression_byte[0] & 0x7f;
        if !matches!(compression, 1 | 2 | 3) {
            return conv(format!(
                "区域文件 {name} 第 {index} 项压缩类型非法（{compression}）"
            ));
        }

        let payload_length = (length - 1) as u64;
        if external {
            validate_external_chunk(path, &name, index, start, payload_length, compression)?;
        } else {
            let mut payload = (&mut file).take(payload_length);
            validate_chunk_nbt(compression, &mut payload, &name, index)?;
        }
        chunks += 1;
    }
    Ok(chunks)
}

/// 外部区块：payload 位于同名 .mcc 的相同扇区偏移处（无 4 字节长度前缀）。
fn validate_external_chunk(
    region_path: &Path,
    name: &str,
    index: usize,
    start: u64,
    payload_length: u64,
    compression: u8,
) -> Result<()> {
    let mcc = mcc_path(region_path).ok_or_else(|| {
        ConversionError(format!("区域文件 {name} 第 {index} 项引用外部区块，但无法推导 .mcc 文件名"))
    })?;
    let mut external = fs::File::open(&mcc).map_err(|_| {
        ConversionError(format!(
            "区域文件 {name} 第 {index} 项外部区块文件缺失：{}",
            mcc.display()
        ))
    })?;
    external.seek(SeekFrom::Start(start))?;
    let mut payload = external.take(payload_length);
    let mut first = [0u8; 1];
    let read = payload
        .read(&mut first)
        .map_err(|_| ConversionError(format!("区域文件 {name} 第 {index} 项外部区块读取失败")))?;
    if read == 0 {
        return conv(format!("区域文件 {name} 第 {index} 项外部区块为空"));
    }
    // .mcc 中该位置可能自带压缩类型字节；两种布局都兼容
    if first[0] == compression {
        validate_chunk_nbt(compression, &mut payload, name, index)?;
    } else {
        let mut chain = Cursor::new(vec![first[0]]).chain(payload);
        validate_chunk_nbt(compression, &mut chain, name, index)?;
    }
    Ok(())
}

fn validate_chunk_nbt(compression: u8, reader: &mut dyn Read, name: &str, index: usize) -> Result<()> {
    let result = match compression {
        1 => nbt::validate_root(GzDecoder::new(reader)),
        2 => nbt::validate_root(ZlibDecoder::new(reader)),
        3 => nbt::validate_root(reader),
        _ => unreachable!(),
    };
    result
        .map_err(|error| ConversionError(format!("区域文件 {name} 第 {index} 项 NBT 校验失败：{error}")))
}

/// r.0.0.mca → c.0.0.mcc（同级目录）。
fn mcc_path(region: &Path) -> Option<PathBuf> {
    let name = region.file_name()?.to_string_lossy().into_owned();
    if !name.to_lowercase().ends_with(".mca") {
        return None;
    }
    let stem = &name[..name.len() - 4];
    let rest = stem.strip_prefix('r').or_else(|| stem.strip_prefix('R'))?;
    if rest.starts_with('.') && rest[1..].contains('.') {
        return Some(region.with_file_name(format!("c{rest}.mcc")));
    }
    None
}

/// 输出文件统计（不计 _NWC_preserved_source 顶层目录）。
fn count_output(world: &Path) -> Result<(i64, i64)> {
    let mut files: i64 = 0;
    let mut bytes: i64 = 0;
    for entry in WalkDir::new(world).into_iter().filter_map(|entry| entry.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(world)
            .map_err(|_| ConversionError("验证内部路径错误".into()))?;
        if relative
            .components()
            .next()
            .is_some_and(|part| part.as_os_str() == "_NWC_preserved_source")
        {
            continue;
        }
        files += 1;
        if let Ok(metadata) = entry.metadata() {
            bytes += metadata.len() as i64;
        }
    }
    Ok((files, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::AppLog;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    use std::sync::atomic::AtomicBool;
    use tauri::Manager;
    use std::sync::Arc;
    use tauri::test::mock_app;

    /// 一个合法的空 Compound NBT。
    fn empty_compound() -> Vec<u8> {
        vec![0x0A, 0x00, 0x00, 0x00]
    }

    fn write_level_dat(dir: &Path) {
        // 根 Compound：Data{ DataVersion=266, LevelName="abc", Version{ Name="1.21" } }
        let mut nbt = Vec::new();
        nbt.extend_from_slice(&[0x0A, 0x00, 0x00]); // root, name ""
        nbt.extend_from_slice(&[0x0A, 0x00, 0x04]);
        nbt.extend_from_slice(b"Data");
        nbt.extend_from_slice(&[0x03, 0x00, 0x0B]);
        nbt.extend_from_slice(b"DataVersion");
        nbt.extend_from_slice(&[0x00, 0x00, 0x01, 0x0A]); // 266
        nbt.extend_from_slice(&[0x08, 0x00, 0x09]);
        nbt.extend_from_slice(b"LevelName");
        nbt.extend_from_slice(&[0x00, 0x03]);
        nbt.extend_from_slice(b"abc");
        nbt.extend_from_slice(&[0x0A, 0x00, 0x07]);
        nbt.extend_from_slice(b"Version");
        nbt.extend_from_slice(&[0x08, 0x00, 0x04]);
        nbt.extend_from_slice(b"Name");
        nbt.extend_from_slice(&[0x00, 0x04]);
        nbt.extend_from_slice(b"1.21");
        nbt.push(0x00); // end Version
        nbt.push(0x00); // end Data
        nbt.push(0x00); // end root
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&nbt).unwrap();
        fs::write(dir.join("level.dat"), encoder.finish().unwrap()).unwrap();
    }

    fn write_test_region(dir: &Path) {
        let nbt = empty_compound();
        let length = (nbt.len() + 1) as u32; // 压缩类型字节 + payload
        let mut header = vec![0u8; 4096];
        header[0..3].copy_from_slice(&[0x00, 0x00, 0x02]); // offset=2（BE24）
        header[3] = 1; // 1 个扇区
        let mut data = vec![0u8; 4096];
        data[0..4].copy_from_slice(&length.to_be_bytes());
        data[4] = 3; // 无压缩
        data[5..5 + nbt.len()].copy_from_slice(&nbt);
        let mut region = Vec::new();
        region.extend_from_slice(&header);
        region.extend_from_slice(&data);
        fs::write(dir.join("r.0.0.mca"), region).unwrap();
    }

    #[test]
    fn validates_minimal_anvil_world() {
        let app = mock_app();
        let dir = tempfile::tempdir().unwrap();
        write_level_dat(dir.path());
        write_test_region(dir.path());
        let log_path = app
            .path()
            .app_log_dir()
            .unwrap()
            .join("test-validate.log");
        let log = Arc::new(AppLog::new(&log_path).unwrap());
        let sink = crate::sink::Sink::new(
            "test".into(),
            Arc::new(AtomicBool::new(false)),
            log,
            |_payload| {},
        );
        let result = validate(dir.path(), &sink).unwrap();
        assert_eq!(result.region_files, 1);
        assert_eq!(result.region_chunks, 1);
        assert_eq!(result.level_version, "1.21");
        assert_eq!(result.data_version, 266);
    }

    #[test]
    fn rejects_overlapping_sectors() {
        let dir = tempfile::tempdir().unwrap();
        write_level_dat(dir.path());
        write_test_region(dir.path());
        // 篡改第二个位置表项，使其与第一项共用扇区 2
        let path = dir.path().join("r.0.0.mca");
        let mut bytes = fs::read(&path).unwrap();
        let base = 4;
        bytes[base..base + 3].copy_from_slice(&[0x00, 0x00, 0x02]);
        bytes[base + 3] = 1;
        fs::write(&path, bytes).unwrap();
        let app = mock_app();
        let log_path = app
            .path()
            .app_log_dir()
            .unwrap()
            .join("test-validate.log");
        let log = Arc::new(AppLog::new(&log_path).unwrap());
        let sink = crate::sink::Sink::new(
            "test".into(),
            Arc::new(AtomicBool::new(false)),
            log,
            |_payload| {},
        );
        assert!(validate(dir.path(), &sink).is_err());
    }
}
