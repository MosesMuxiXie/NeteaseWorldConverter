// 对应 NeteaseBedrockDecryptor.java：网易基岩 80 1D 30 01 循环 XOR 解密与 LevelDB 规范化。

use crate::archive::file_name;
use crate::detect::{list_regular_files, starts_with_header, LEGACY_AES_HEADER, NETEASE_HEADER};
use crate::error::{conv, Result};
use crate::log::AppLog;
use crate::models::{WorldInfo, WorldType};
use crate::sink::Sink;
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

const LEVELDB_FOOTER: [u8; 8] = [0x57, 0xFB, 0x80, 0x8B, 0x24, 0x75, 0x47, 0xDB];

pub fn prepare(world: &WorldInfo, output: &Path, sink: &Sink) -> Result<()> {
    let log = &sink.log;
    if !world.world_type.is_bedrock() {
        return conv("内部错误：尝试把 Java 世界作为 Bedrock 解密");
    }
    if world.world_type == WorldType::NeteaseBedrockLegacyAes {
        return conv(
            "检测到旧版 90 1D 30 01 AES-CFB8 加密。密钥不在存档中，无法离线恢复。\
请使用仍能打开该世界的网易客户端重新导出，或提供对应账号 API 返回的密钥。",
        );
    }
    let source_root = &world.root;
    let Some(source_db) = world.database_directory.as_ref().filter(|db| db.is_dir()) else {
        return conv("基岩数据库目录缺失");
    };
    fs::create_dir_all(output)?;
    let output_db = output.join("db");
    fs::create_dir_all(&output_db)?;

    copy_world_metadata(source_root, source_db, output, sink)?;

    let root_level = source_root.join("level.dat");
    let nested_level = source_db.join("level.dat");
    if root_level.is_file() {
        fs::copy(&root_level, output.join("level.dat"))?;
    } else if nested_level.is_file() {
        fs::copy(&nested_level, output.join("level.dat"))?;
    } else {
        return conv("基岩世界缺少 level.dat（根目录和数据库目录均未找到）");
    }
    let old_level = source_root.join("level.dat_old");
    if old_level.is_file() {
        fs::copy(&old_level, output.join("level.dat_old"))?;
    }

    let db_files = collect_database_files(source_root, source_db);
    let manifest_name = choose_manifest_name(&db_files)?;
    let encrypted = db_files
        .values()
        .any(|path| starts_with_header(path, &NETEASE_HEADER));
    let legacy = db_files
        .values()
        .any(|path| starts_with_header(path, &LEGACY_AES_HEADER));
    if legacy {
        return conv("数据库中混有旧版 AES-CFB8 文件，不能安全解密");
    }

    let mut key: Option<Vec<u8>> = None;
    if encrypted {
        let recovery = recover_key(&db_files, &manifest_name, log)?;
        log.info(&format!(
            "恢复网易 XOR 密钥：{}；LevelDB footer 一致 {}/{}",
            crate::archive::hex(&recovery.key),
            recovery.valid_footers,
            recovery.encrypted_ldb
        ));
        key = Some(recovery.key);
    }

    let ordered: Vec<(String, PathBuf)> = db_files
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let total = ordered.len();
    let processed = AtomicUsize::new(0);
    let decrypted_files = AtomicUsize::new(0);
    let copied_files = AtomicUsize::new(0);
    // 多个大 .ldb 文件相互独立，按文件并行解密/复制
    ordered
        .par_iter()
        .try_for_each::<_, Result<()>>(|(name, source)| {
            sink.check_cancel()?;
            let lower = name.to_lowercase();
            if lower == "level.dat" || lower == ".ds_store" || lower == "lock" {
                let _ = processed.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            let target = output_db.join(name);
            if starts_with_header(source, &NETEASE_HEADER) {
                let Some(key_bytes) = key.as_ref() else {
                    return conv(format!("发现加密文件但未恢复出密钥：{name}"));
                };
                decrypt_file(source, &target, key_bytes)?;
                let _ = decrypted_files.fetch_add(1, Ordering::Relaxed);
            } else {
                fs::copy(source, &target)?;
                let _ = copied_files.fetch_add(1, Ordering::Relaxed);
            }
            let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
            let percent = 13 + (17i64 * done as i64 / total.max(1) as i64) as i32;
            sink.update(
                percent,
                if encrypted {
                    "解密网易 LevelDB"
                } else {
                    "规范化 Bedrock"
                },
                &format!("{done} / {total} 个数据库文件"),
            );
            Ok(())
        })?;
    let decrypted_files = decrypted_files.load(Ordering::Relaxed);
    let copied_files = copied_files.load(Ordering::Relaxed);

    fs::write(output_db.join("CURRENT"), format!("{manifest_name}\n"))?;
    fs::write(output_db.join("LOCK"), b"")?;
    let mut validated = 0usize;
    if let Ok(entries) = fs::read_dir(&output_db) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if path.is_file() && file_name(&path).to_lowercase().ends_with(".ldb") {
                if !has_plain_footer(&path) {
                    return conv(format!(
                        "解密后的 LevelDB footer 校验失败：{}",
                        file_name(&path)
                    ));
                }
                validated += 1;
            }
        }
    }
    if let Ok(entries) = fs::read_dir(&output_db) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            if starts_with_header(&entry.path(), &NETEASE_HEADER) {
                return conv("规范化后的数据库仍残留网易加密头");
            }
        }
    }
    log.info(&format!(
        "Bedrock 准备完成：解密 {decrypted_files}，直接复制 {copied_files}，有效 LDB {validated}"
    ));
    Ok(())
}

fn copy_world_metadata(
    source_root: &Path,
    source_db: &Path,
    output: &Path,
    sink: &Sink,
) -> Result<()> {
    fn visit(dir: &Path, source_root: &Path, source_db: &Path, output: &Path) -> Result<()> {
        for entry in fs::read_dir(dir)?.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if path == source_db || path == output || path.starts_with(output) {
                continue;
            }
            let relative = path
                .strip_prefix(source_root)
                .map_err(|_| crate::error::ConversionError::from("内部路径错误"))?;
            if relative.iter().any(|part| {
                ["_conversion", "__MACOSX", ".git"].contains(&part.to_string_lossy().as_ref())
            }) {
                continue;
            }
            let target = output.join(relative);
            if path.is_dir() {
                fs::create_dir_all(&target)?;
                visit(&path, source_root, source_db, output)?;
            } else {
                let name = file_name(&path);
                let lower = name.to_lowercase();
                let is_root_db_artifact = relative.components().count() == 1
                    && (name.to_uppercase().starts_with("MANIFEST-")
                        || name.to_uppercase() == "CURRENT"
                        || name.to_uppercase() == "LOCK"
                        || name.to_uppercase().ends_with(".LDB")
                        || name.to_uppercase().ends_with(".LOG"));
                if is_root_db_artifact {
                    continue;
                }
                if lower == "level.dat" || lower == "level.dat_old" {
                    continue;
                }
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&path, &target)?;
            }
        }
        Ok(())
    }
    visit(source_root, source_root, source_db, output)?;
    sink.update(13, "准备 Bedrock", "世界元数据与数据包已复制");
    Ok(())
}

fn collect_database_files(root: &Path, db: &Path) -> BTreeMap<String, PathBuf> {
    let mut files = list_regular_files(db);
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = file_name(&path).to_uppercase();
            let is_artifact = name.starts_with("MANIFEST-")
                || name == "CURRENT"
                || name == "LOCK"
                || name.ends_with(".LDB")
                || name.ends_with(".LOG");
            if is_artifact {
                files.entry(file_name(&path)).or_insert(path);
            }
        }
    }
    files
}

fn manifest_number(name: &str) -> Option<u64> {
    name.strip_prefix("MANIFEST-")
        .and_then(|suffix| suffix.parse().ok())
}

fn choose_manifest_name(files: &BTreeMap<String, PathBuf>) -> Result<String> {
    // MANIFEST-<编号> 按 LevelDB 编号取数值最大；字典序在非零填充编号时会选错
    // （如 MANIFEST-10 < MANIFEST-2），无法解析编号的回退为字典序末位。
    let newest = files
        .keys()
        .filter(|name| name.starts_with("MANIFEST-"))
        .max_by_key(|name| manifest_number(name).unwrap_or(0))
        .ok_or_else(|| crate::error::ConversionError::from("LevelDB 缺少 MANIFEST-* 文件"))?;
    if let Some(current) = files.get("CURRENT") {
        if !starts_with_header(current, &NETEASE_HEADER) {
            if let Ok(value) = fs::read_to_string(current) {
                let value = value.trim();
                if !value.is_empty() && files.contains_key(value) {
                    return Ok(value.to_string());
                }
            }
        }
    }
    Ok(newest.clone())
}

struct KeyRecovery {
    key: Vec<u8>,
    encrypted_ldb: usize,
    valid_footers: usize,
}

fn recover_key(
    files: &BTreeMap<String, PathBuf>,
    manifest_name: &str,
    log: &AppLog,
) -> Result<KeyRecovery> {
    let mut candidates: Vec<Vec<u8>> = Vec::new();
    if let Some(current) = files.get("CURRENT") {
        if starts_with_header(current, &NETEASE_HEADER) {
            let encrypted = fs::read(current)?;
            let plain = format!("{manifest_name}\n").into_bytes();
            if encrypted.len() as i64 - NETEASE_HEADER.len() as i64 == plain.len() as i64 {
                let mut raw = vec![0u8; plain.len()];
                for (index, byte) in plain.iter().enumerate() {
                    raw[index] = encrypted[index + NETEASE_HEADER.len()] ^ byte;
                }
                candidates.push(shortest_period(&raw));
            }
        }
    }

    let mut ldb_candidates: BTreeMap<String, usize> = BTreeMap::new();
    let mut encrypted_ldb = 0usize;
    for (name, path) in files {
        if !name.to_lowercase().ends_with(".ldb") || !starts_with_header(path, &NETEASE_HEADER) {
            continue;
        }
        encrypted_ldb += 1;
        if let Some(candidate) = recover_eight_byte_key_from_footer(path)? {
            *ldb_candidates
                .entry(crate::archive::hex(&candidate))
                .or_insert(0) += 1;
            candidates.push(candidate);
        }
    }
    if candidates.is_empty() {
        return conv("无法从 CURRENT 或 .ldb footer 恢复网易 XOR 密钥");
    }

    let mut best: Option<Vec<u8>> = None;
    let mut best_valid = -1i64;
    for candidate in &candidates {
        let valid = count_valid_encrypted_footers(files, candidate)? as i64;
        if valid > best_valid {
            best_valid = valid;
            best = Some(candidate.clone());
        }
    }
    let best = best.unwrap();
    if encrypted_ldb > 0 && best_valid == 0 {
        return conv("候选密钥无法通过任何 LevelDB footer 校验");
    }
    if encrypted_ldb > 0 && best_valid != encrypted_ldb as i64 {
        log.warn(&format!(
            "仅有 {best_valid}/{encrypted_ldb} 个加密 LDB footer 与恢复密钥一致"
        ));
    }
    Ok(KeyRecovery {
        key: best,
        encrypted_ldb,
        valid_footers: best_valid as usize,
    })
}

fn read_tail(path: &Path, count: usize) -> Result<Vec<u8>> {
    let size = fs::metadata(path)?.len();
    let mut file = fs::File::open(path)?;
    std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(size - count as u64))?;
    let mut tail = vec![0u8; count];
    file.read_exact(&mut tail)?;
    Ok(tail)
}

fn recover_eight_byte_key_from_footer(file: &Path) -> Result<Option<Vec<u8>>> {
    let size = fs::metadata(file)?.len();
    let plain_length = size as i64 - NETEASE_HEADER.len() as i64;
    if plain_length < LEVELDB_FOOTER.len() as i64 {
        return Ok(None);
    }
    let tail = read_tail(file, LEVELDB_FOOTER.len())?;
    let plain_offset = plain_length - LEVELDB_FOOTER.len() as i64;
    // 密钥 8 字节按环形相位放置：key[(plainOffset+i) % 8] = tail[i] ^ footer[i]
    let mut key = [0u8; 8];
    for index in 0..8 {
        let key_index = ((plain_offset + index as i64) % 8) as usize;
        key[key_index] = tail[index] ^ LEVELDB_FOOTER[index];
    }
    Ok(Some(key.to_vec()))
}

fn encrypted_footer_matches(file: &Path, key: &[u8]) -> Result<bool> {
    let size = fs::metadata(file)?.len();
    let plain_length = size as i64 - NETEASE_HEADER.len() as i64;
    if plain_length < 8 {
        return Ok(false);
    }
    let tail = read_tail(file, 8)?;
    let offset = plain_length - 8;
    for index in 0..8 {
        let plain = tail[index] ^ key[((offset + index as i64) % key.len() as i64) as usize];
        if plain != LEVELDB_FOOTER[index] {
            return Ok(false);
        }
    }
    Ok(true)
}

fn count_valid_encrypted_footers(files: &BTreeMap<String, PathBuf>, key: &[u8]) -> Result<usize> {
    let mut valid = 0;
    for (name, path) in files {
        if name.to_lowercase().ends_with(".ldb")
            && starts_with_header(path, &NETEASE_HEADER)
            && encrypted_footer_matches(path, key)?
        {
            valid += 1;
        }
    }
    Ok(valid)
}

fn shortest_period(raw: &[u8]) -> Vec<u8> {
    for period in 1..=raw.len().min(32) {
        let mut matched = true;
        for index in period..raw.len() {
            if raw[index] != raw[index % period] {
                matched = false;
                break;
            }
        }
        if matched {
            return raw[..period].to_vec();
        }
    }
    raw.to_vec()
}

fn decrypt_file(source: &Path, target: &Path, key: &[u8]) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut input = BufReader::new(fs::File::open(source)?);
    let mut output = BufWriter::new(fs::File::create(target)?);
    let mut header = [0u8; 4];
    input.read_exact(&mut header)?;
    if header != NETEASE_HEADER {
        return conv(format!(
            "加密文件头在读取过程中发生变化：{}",
            source.display()
        ));
    }
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut position: u64 = 0;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        xor_slice(&mut buffer[..read], key, position);
        output.write_all(&buffer[..read])?;
        position += read as u64;
    }
    Ok(())
}

/// 循环 XOR：密钥按全局明文偏移环形取用。
/// key.len() 为 8 的倍数时按 u64 字块处理（打包后 XOR 与逐字节 XOR 等价），
/// 大文件上明显快于逐字节取模；其余长度回退逐字节。
fn xor_slice(buffer: &mut [u8], key: &[u8], position: u64) {
    if key.is_empty() {
        return;
    }
    let key_len = key.len();
    if key_len % 8 != 0 {
        let mut pos = (position % key_len as u64) as usize;
        for byte in buffer.iter_mut() {
            *byte ^= key[pos];
            pos = (pos + 1) % key_len;
        }
        return;
    }
    let words: Vec<u64> = key
        .chunks_exact(8)
        .map(|chunk| u64::from_be_bytes(chunk.try_into().expect("key 长度为 8 的倍数")))
        .collect();
    let block_words = words.len();
    let mut pos = (position % key_len as u64) as usize;
    let mut index = 0;
    // 先按字节对齐到密钥周期边界（也即 8 字节边界）
    while index < buffer.len() && pos % 8 != 0 {
        buffer[index] ^= key[pos];
        pos += 1;
        index += 1;
    }
    pos %= key_len;
    let mut word_index = pos / 8;
    let aligned = buffer.len() - index;
    let full = aligned - aligned % 8;
    for chunk in buffer[index..index + full].chunks_exact_mut(8) {
        let current = u64::from_be_bytes(chunk.try_into().expect("8 字节"));
        chunk.copy_from_slice((current ^ words[word_index]).to_be_bytes().as_ref());
        word_index = (word_index + 1) % block_words;
    }
    let tail_start = word_index * 8;
    for (offset, byte) in buffer[index + full..].iter_mut().enumerate() {
        *byte ^= key[tail_start + offset];
    }
}

fn has_plain_footer(file: &Path) -> bool {
    let Ok(size) = fs::metadata(file).map(|m| m.len()) else {
        return false;
    };
    if size < 8 {
        return false;
    }
    match read_tail(file, 8) {
        Ok(tail) => tail == LEVELDB_FOOTER,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::peek_legacy_aes;
    use crate::log::AppLog;
    use crate::models::{WorldInfo, WorldType};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    fn xor_encrypt(plain: &[u8], key: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + plain.len());
        out.extend_from_slice(&NETEASE_HEADER);
        for (index, byte) in plain.iter().enumerate() {
            out.push(byte ^ key[index % key.len()]);
        }
        out
    }

    fn build_world(dir: &Path, key: &[u8]) {
        let db = dir.join("db");
        fs::create_dir_all(&db).unwrap();
        fs::write(dir.join("level.dat"), b"bedrock-level").unwrap();
        let manifest_plain = b"MANIFEST-000001\n".to_vec();
        fs::write(
            db.join("MANIFEST-000001"),
            xor_encrypt(&manifest_plain, key),
        )
        .unwrap();
        fs::write(db.join("CURRENT"), xor_encrypt(b"MANIFEST-000001\n", key)).unwrap();
        // 明文 LDB 长度必须是 8 的倍数，且末尾 8 字节为 footer 魔数
        let mut ldb = vec![0x42u8; 1024];
        ldb[1016..].copy_from_slice(&LEVELDB_FOOTER);
        fs::write(db.join("000001.ldb"), xor_encrypt(&ldb, key)).unwrap();
    }

    fn test_sink(dir: &Path) -> Sink {
        let log = Arc::new(AppLog::new(&dir.join("test-conversion.log")).unwrap());
        Sink::new(
            "test".into(),
            Arc::new(AtomicBool::new(false)),
            log,
            |_payload| {},
        )
    }

    #[test]
    fn decrypts_netease_leveldb_and_normalizes() {
        let key = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        // 输出目录必须与世界根分离——与真实用法（work/bedrock 与 extracted/…）及 Java 原版一致
        let world_dir = tempfile::tempdir().unwrap();
        let output_dir = tempfile::tempdir().unwrap();
        build_world(world_dir.path(), &key);
        let world = WorldInfo {
            root: world_dir.path().to_path_buf(),
            database_directory: Some(world_dir.path().join("db")),
            world_type: WorldType::NeteaseBedrock,
            detected_version: "基岩 LevelDB".into(),
            world_name: "test".into(),
            file_count: 0,
            byte_count: 0,
            notes: vec![],
        };
        let sink = test_sink(world_dir.path());
        let output = output_dir.path().join("bedrock");
        crate::decrypt::prepare(&world, &output, &sink).unwrap();

        let ldb = fs::read(output.join("db/000001.ldb")).unwrap();
        assert_eq!(ldb.len(), 1024);
        assert_eq!(&ldb[1016..], &LEVELDB_FOOTER);
        assert_eq!(
            fs::read_to_string(output.join("db/CURRENT")).unwrap(),
            "MANIFEST-000001\n"
        );
        assert_eq!(
            fs::read(output.join("level.dat")).unwrap(),
            b"bedrock-level"
        );
    }

    #[test]
    fn shortest_period_finds_key_from_current() {
        let key = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut raw = Vec::new();
        for i in 0..17 {
            raw.push(key[i % 8]);
        }
        assert_eq!(shortest_period(&raw), key.to_vec());
    }

    #[test]
    fn xor_slice_matches_naive_for_all_key_lengths() {
        // 字块快速路径必须与朴素逐字节实现在任意 key 长度 / 起始相位下等价
        fn naive(buffer: &mut [u8], key: &[u8], position: u64) {
            for (index, byte) in buffer.iter_mut().enumerate() {
                *byte ^= key[((position + index as u64) % key.len() as u64) as usize];
            }
        }
        let mut data = vec![0u8; 1000];
        let mut seed = 0x1234_5678u32;
        for byte in data.iter_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *byte = (seed >> 24) as u8;
        }
        let key_lengths = [1usize, 3, 5, 7, 8, 9, 16, 17, 32];
        let positions = [0u64, 1, 7, 8, 16, 17, 33, 123_456_789];
        for key_len in key_lengths {
            let key: Vec<u8> = (0..key_len).map(|i| (i * 7 + 3) as u8).collect();
            for position in positions {
                let mut fast = data.clone();
                xor_slice(&mut fast, &key, position);
                let mut expected = data.clone();
                naive(&mut expected, &key, position);
                assert_eq!(fast, expected, "key_len={key_len} position={position}");
            }
        }
    }

    #[test]
    fn choose_manifest_prefers_highest_number() {
        // 非零填充编号下字典序会选错（MANIFEST-10 < MANIFEST-2）
        let dir = tempfile::tempdir().unwrap();
        let mut files = BTreeMap::new();
        for name in ["MANIFEST-2", "MANIFEST-10", "MANIFEST-000003"] {
            let path = dir.path().join(name);
            fs::write(&path, b"").unwrap();
            files.insert(name.to_string(), path);
        }
        assert_eq!(choose_manifest_name(&files).unwrap(), "MANIFEST-10");
    }

    #[test]
    fn peek_legacy_aes_detects_without_extraction() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("world.zip");
        {
            use std::io::Write as _;
            let file = fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            let mut entry = LEGACY_AES_HEADER.to_vec();
            entry.extend_from_slice(&xor_encrypt(&[0x42u8; 64], &[1, 2, 3, 4, 5, 6, 7, 8])[4..]);
            zip.start_file("world/db/000001.ldb", options).unwrap();
            zip.write_all(&entry).unwrap();
            zip.finish().unwrap();
        }
        assert!(peek_legacy_aes(&zip_path));

        // 无 db 条目的普通 zip → 未嗅探到
        let plain_zip = dir.path().join("plain.zip");
        {
            use std::io::Write as _;
            let file = fs::File::create(&plain_zip).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("hello.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"hi").unwrap();
            zip.finish().unwrap();
        }
        assert!(!peek_legacy_aes(&plain_zip));
    }
}
