// 对应 ArchiveTools.java：带安全限制的 ZIP 解压/打包、目录复制删除、哈希。

use crate::detect::{matches_db_directory, LEGACY_AES_HEADER, NETEASE_HEADER};
use crate::error::{conv, ConversionError, Result};
use crate::sink::Sink;
use chrono::{DateTime, Datelike, Local, Timelike};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

const MAX_UNCOMPRESSED_BYTES: u64 = 256 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 1_000_000;

pub const IGNORED: [&str; 3] = ["__MACOSX", ".git", "_conversion"];

pub fn is_ignored_path(path: &Path, root: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    relative
        .iter()
        .any(|part| IGNORED.contains(&part.to_string_lossy().as_ref()))
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub fn strip_extension(name: &str) -> String {
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[..dot].to_string(),
        _ => name.to_string(),
    }
}

pub fn sha256(path: &Path) -> Result<String> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex(&digest.finalize()))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

pub fn safe_folder_name(name: &str) -> String {
    let mut cleaned: String = name
        .chars()
        .map(|c| {
            let invalid = matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                || (c as u32) < 0x20
                || c == '\u{7F}';
            if invalid {
                '_'
            } else {
                c
            }
        })
        .collect();
    cleaned = cleaned.trim().to_string();
    while cleaned.ends_with('.') || cleaned.ends_with(' ') {
        cleaned.pop();
    }
    if cleaned.is_empty() {
        "ConvertedWorld".to_string()
    } else {
        cleaned
    }
}

pub fn delete_tree(root: &Path) {
    if root.as_os_str().is_empty() || !root.exists() {
        return;
    }
    let _ = fs::remove_dir_all(root);
}

fn sanitize_entry_name(name: &str) -> Result<String> {
    let mut normalized = name.replace('\\', "/");
    while normalized.starts_with("./") {
        normalized = normalized[2..].to_string();
    }
    let drive_path = normalized
        .as_bytes()
        .first()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && normalized.as_bytes().get(1) == Some(&b':');
    if normalized.starts_with('/') || drive_path {
        return conv(format!("ZIP 包含绝对路径：{name}"));
    }
    if normalized.split('/').any(|part| part == "..") {
        return conv(format!("ZIP 包含父目录跳转：{name}"));
    }
    Ok(normalized)
}

pub fn extract_zip(zip_path: &Path, destination: &Path, sink: &Sink) -> Result<(u64, u64)> {
    fs::create_dir_all(destination)?;
    let destination_real = destination.canonicalize()?;
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))?;

    let mut declared_bytes: u64 = 0;
    let mut declared_files: usize = 0;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if !entry.is_dir() {
            declared_files += 1;
            if entry.size() > 0 {
                declared_bytes = declared_bytes
                    .checked_add(entry.size())
                    .ok_or_else(|| ConversionError::from("ZIP 声明的数据大小溢出"))?;
            }
        }
        if declared_files > MAX_ENTRIES || declared_bytes > MAX_UNCOMPRESSED_BYTES {
            return conv("ZIP 规模超过安全限制（最多 100 万文件、256 GiB 解压数据）");
        }
    }

    let mut written: HashSet<PathBuf> = HashSet::new();
    let mut extracted_bytes: u64 = 0;
    let mut extracted_files: usize = 0;
    for index in 0..archive.len() {
        sink.check_cancel()?;
        let mut entry = archive.by_index(index)?;
        let clean_name = sanitize_entry_name(entry.name())?;
        if clean_name.is_empty() {
            continue;
        }
        let output = destination_real.join(&clean_name);
        if !output.starts_with(&destination_real) {
            return conv(format!("ZIP 包含越界路径：{}", entry.name()));
        }
        if !written.insert(output.clone()) {
            return conv(format!("ZIP 包含重复路径：{}", entry.name()));
        }
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut target = BufWriter::new(fs::File::create(&output)?);
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = entry.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            target.write_all(&buffer[..read])?;
            extracted_bytes += read as u64;
            if extracted_bytes > MAX_UNCOMPRESSED_BYTES {
                return conv("ZIP 实际解压数据超过 256 GiB 安全限制");
            }
        }
        drop(target);
        extracted_files += 1;
        let percent = if declared_bytes > 0 {
            2 + (extracted_bytes * 8)
                .checked_div(declared_bytes)
                .unwrap_or(0)
                .min(8) as i32
        } else {
            2 + (((extracted_files as u64) * 8 / declared_files.max(1) as u64).min(8) as i32)
        };
        sink.update(
            percent,
            "安全解压",
            &format!("{extracted_files} / {declared_files} 个文件"),
        );
    }
    sink.log.info(&format!(
        "ZIP 解压完成：{extracted_files} 个文件，{extracted_bytes} 字节"
    ));
    Ok((extracted_files as u64, extracted_bytes))
}

pub fn create_zip(world: &Path, output_zip: &Path, folder_name: &str, sink: &Sink) -> Result<()> {
    let mut files: Vec<PathBuf> = WalkDir::new(world)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect();
    files.sort();
    let mut total_bytes: u64 = 0;
    for file in &files {
        total_bytes += fs::metadata(file)?.len();
    }
    if let Some(parent) = output_zip.parent() {
        fs::create_dir_all(parent)?;
    }
    let output = fs::File::create(output_zip)?;
    let mut zip = zip::ZipWriter::new(BufWriter::new(output));
    let mut written: u64 = 0;
    for (index, file) in files.iter().enumerate() {
        sink.check_cancel()?;
        let relative = file
            .strip_prefix(world)
            .map_err(|_| ConversionError::from("内部路径错误"))?
            .to_string_lossy()
            .replace('\\', "/");
        let mut options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(1))
            .large_file(true);
        if let Ok(modified) = fs::metadata(file).and_then(|m| m.modified()) {
            let datetime: DateTime<Local> = modified.into();
            if let Some(entry_time) = zip_datetime(&datetime) {
                options = options.last_modified_time(entry_time);
            }
        }
        zip.start_file(format!("{folder_name}/{relative}"), options)?;
        let mut input = BufReader::new(fs::File::open(file)?);
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            zip.write_all(&buffer[..read])?;
            written += read as u64;
        }
        let percent = 94
            + (if total_bytes > 0 {
                (written * 6).checked_div(total_bytes).unwrap_or(0).min(6)
            } else {
                (((index + 1) as u64) * 6 / files.len().max(1) as u64).min(6)
            }) as i32;
        sink.update(
            percent,
            "打包 ZIP",
            &format!("{} / {} 个文件", index + 1, files.len()),
        );
    }
    zip.finish()?;
    let size = fs::metadata(output_zip)?.len();
    sink.log.info(&format!(
        "输出 ZIP：{}（{size} 字节）",
        output_zip.display()
    ));
    Ok(())
}

fn zip_datetime(datetime: &DateTime<Local>) -> Option<zip::DateTime> {
    zip::DateTime::from_date_and_time(
        datetime.year().max(1980) as u16,
        datetime.month() as u8,
        datetime.day() as u8,
        datetime.hour() as u8,
        datetime.minute() as u8,
        datetime.second() as u8,
    )
    .ok()
}

pub fn copy_tree(
    source: &Path,
    destination: &Path,
    sink: &Sink,
    start: i32,
    end: i32,
) -> Result<()> {
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
            .map_err(|_| ConversionError::from("内部路径错误"))?;
        let target = destination.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(file, &target)?;
        let percent = start
            + (((end - start) as i64) * ((index + 1) as i64) / files.len().max(1) as i64) as i32;
        sink.update(
            percent,
            "复制存档",
            &format!("{} / {} 个文件", index + 1, files.len()),
        );
    }
    Ok(())
}

/// 解压前嗅探：仅扫描 ZIP central directory，对疑似 LevelDB 数据库条目（db 目录下的
/// .ldb/.log/MANIFEST-*/CURRENT）读取前 4 字节头部。返回 true 表示嗅探到旧版
/// AES-CFB8（90 1D 30 01）且未见新版网易头（80 1D 30 01）——此类存档无法离线解密，
/// 调用方可跳过全量解压直接失败。任何异常按"未嗅探到"处理，走常规全量解压路径；
/// 该结果仅为快速通道，权威判定仍以解压后的 detect 为准。
pub fn peek_legacy_aes(zip_path: &Path) -> bool {
    let Ok(file) = fs::File::open(zip_path) else {
        return false;
    };
    let Ok(mut archive) = zip::ZipArchive::new(BufReader::new(file)) else {
        return false;
    };
    let mut saw_legacy = false;
    let mut saw_modern = false;
    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else {
            return false;
        };
        if entry.is_dir() || entry.size() < 4 {
            continue;
        }
        let cleaned = entry.name().replace('\\', "/");
        let parts: Vec<&str> = cleaned.split('/').collect();
        // 仅认 db/<file> 一层的 LevelDB 数据文件
        let Some(db_position) = parts.iter().position(|part| matches_db_directory(part)) else {
            continue;
        };
        if parts.len() != db_position + 2 {
            continue;
        }
        let entry_name = parts[db_position + 1].to_uppercase();
        let is_leveldb_file = entry_name.ends_with(".LDB")
            || entry_name.ends_with(".LOG")
            || entry_name.starts_with("MANIFEST-")
            || entry_name == "CURRENT";
        if !is_leveldb_file {
            continue;
        }
        let mut header = [0u8; 4];
        let mut filled = 0;
        while filled < 4 {
            match entry.read(&mut header[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(_) => break,
            }
        }
        if filled < 4 {
            continue;
        }
        if header == LEGACY_AES_HEADER {
            saw_legacy = true;
        } else if header == NETEASE_HEADER {
            saw_modern = true;
        }
    }
    saw_legacy && !saw_modern
}

/// 检查目标路径所在卷的可用空间是否充足；无法确定所在卷时放行（宽松策略）。
pub fn ensure_free_space(target: &Path, need: u64, purpose: &str) -> Result<()> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut best: Option<(usize, u64)> = None;
    for disk in &disks {
        let mount = disk.mount_point();
        if target.starts_with(mount) {
            let length = mount.as_os_str().len();
            if best.is_none() || best.is_some_and(|(best_len, _)| length > best_len) {
                best = Some((length, disk.available_space()));
            }
        }
    }
    if let Some((_, available)) = best {
        if available < need {
            return conv(format!(
                "{purpose} 所在磁盘空间不足：需要约 {}，仅剩 {}",
                format_bytes(need),
                format_bytes(available)
            ));
        }
    }
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.2} {}", units[unit])
}
