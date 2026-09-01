// pipeline-bench.rs — 转换流水线各阶段吞吐量基准（开发工具，不随应用发布）。
// 运行：cargo run --release --example pipeline-bench
// 用库的公开 API（extract_zip / create_zip / decrypt::prepare / validate::validate）
// 在 64/256/1024 MiB 三档规模下实测各阶段耗时，验证随大小的线性扩展。

use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use nwc_lib::archive::{create_zip, extract_zip};
use nwc_lib::decrypt;
use nwc_lib::log::AppLog;
use nwc_lib::models::{WorldInfo, WorldType};
use nwc_lib::sink::Sink;
use nwc_lib::validate;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

const NETEASE_HEADER: [u8; 4] = [0x80, 0x1D, 0x30, 0x01];
const LEVELDB_FOOTER: [u8; 8] = [0x57, 0xFB, 0x80, 0x8B, 0x24, 0x75, 0x47, 0xDB];

const SIZES_MIB: [usize; 3] = [64, 256, 1024];
const MIB: usize = 1024 * 1024;

struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_exact_mut(8) {
            chunk.copy_from_slice(&self.next().to_le_bytes());
        }
    }
}

fn make_sink(dir: &Path) -> Sink {
    let log = Arc::new(AppLog::new(&dir.join("bench.log")).unwrap());
    Sink::new(
        "bench".into(),
        Arc::new(AtomicBool::new(false)),
        log,
        |_| {},
    )
}

fn mb(bytes: usize) -> usize {
    bytes * MIB
}

/// 生成混合内容目录：一半伪随机（不可压缩，模拟真实区域数据）、一半全零（可压缩）。
fn build_content_dir(dir: &Path, total: usize, files: usize) {
    fs::create_dir_all(dir).unwrap();
    let mut rng = Xorshift(0x9E37_79B9_7F4A_7C15);
    let mut buffer = vec![0u8; 4 * MIB];
    let per_file = total / files;
    for index in 0..files {
        let path = dir.join(format!("data-{index:03}.bin"));
        let mut file = fs::File::create(&path).unwrap();
        let mut remaining = per_file;
        while remaining > 0 {
            let take = remaining.min(buffer.len());
            let slice = &mut buffer[..take];
            if index % 2 == 0 {
                rng.fill(slice);
            } else {
                slice.fill(0);
            }
            file.write_all(slice).unwrap();
            remaining -= take;
        }
    }
}

/// 合成网易加密 LevelDB：16 个加密 .ldb（模拟真实世界的多文件布局，供并行解密）。
fn build_encrypted_world(root: &Path, total: usize) {
    let db = root.join("db");
    fs::create_dir_all(&db).unwrap();
    fs::write(root.join("level.dat"), b"bench-level").unwrap();
    let key: [u8; 8] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let manifest = b"MANIFEST-000001\n".to_vec();
    let encrypt = |plain: &[u8]| -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + plain.len());
        out.extend_from_slice(&NETEASE_HEADER);
        for (index, byte) in plain.iter().enumerate() {
            out.push(byte ^ key[index % 8]);
        }
        out
    };
    fs::write(db.join("MANIFEST-000001"), encrypt(&manifest)).unwrap();
    fs::write(db.join("CURRENT"), encrypt(&manifest)).unwrap();

    let files = 16;
    let per_file = total / files;
    let mut rng = Xorshift(0xDEAD_BEEF_CAFE_1234);
    let mut buffer = vec![0u8; 4 * MIB];
    for index in 0..files {
        let mut plain_len = per_file - (per_file % 8);
        if index == files - 1 {
            plain_len -= 8; // 留出 footer
        }
        let mut plain = Vec::with_capacity(plain_len + 8);
        while plain.len() < plain_len {
            let take = (plain_len - plain.len()).min(buffer.len());
            rng.fill(&mut buffer[..take]);
            plain.extend_from_slice(&buffer[..take]);
        }
        plain.extend_from_slice(&LEVELDB_FOOTER);
        fs::write(db.join(format!("{index:06}.ldb")), encrypt(&plain)).unwrap();
    }
}

/// 合成 Anvil 世界：N 个区域 × 1024 chunk，chunk 载荷为 zlib 压缩的随机 Byte_Array。
fn build_anvil_world(root: &Path, total: usize) -> (usize, usize) {
    fs::create_dir_all(root.join("region")).unwrap();
    fs::write(root.join("level.dat"), gz_level_dat()).unwrap();

    let mut rng = Xorshift(0x0123_4567_89AB_CDEF);
    let payload_len = 8192;
    let mut raw_payload = vec![0u8; payload_len];
    let mut compressed = Vec::new();
    // chunk NBT：根 Compound + TAG_Byte_Array("Data") + 结束
    {
        let mut nbt = Vec::with_capacity(payload_len + 16);
        nbt.extend_from_slice(&[0x0A, 0x00, 0x00]);
        nbt.extend_from_slice(&[0x07, 0x00, 0x04]);
        nbt.extend_from_slice(b"Data");
        nbt.extend_from_slice(&(payload_len as i32).to_be_bytes());
        rng.fill(&mut raw_payload);
        nbt.extend_from_slice(&raw_payload);
        nbt.push(0x00);
        let mut encoder = ZlibEncoder::new(&mut compressed, Compression::fast());
        encoder.write_all(&nbt).unwrap();
        encoder.finish().unwrap();
    }
    let record_len = 4 + 1 + compressed.len();
    let sectors_per_chunk = record_len.div_ceil(4096) as u32;
    const CHUNKS_PER_REGION: usize = 1024;
    let region_size = 4096 * (2 + CHUNKS_PER_REGION * sectors_per_chunk as usize);
    let regions = (total / region_size).max(1);

    for region_index in 0..regions {
        let mut region = vec![0u8; region_size];
        for chunk_index in 0..CHUNKS_PER_REGION {
            let slot = chunk_index * sectors_per_chunk as usize;
            let offset_bytes = 4096 * (2 + slot);
            // 位置表项：3 字节大端扇区偏移 + 1 字节扇区数
            region[chunk_index * 4..chunk_index * 4 + 3]
                .copy_from_slice(&((2 + slot) as u32).to_be_bytes()[1..4]);
            region[chunk_index * 4 + 3] = sectors_per_chunk as u8;
            region[offset_bytes..offset_bytes + 4]
                .copy_from_slice(&((compressed.len() + 1) as u32).to_be_bytes());
            region[offset_bytes + 4] = 2; // zlib
            region[offset_bytes + 5..offset_bytes + 5 + compressed.len()]
                .copy_from_slice(&compressed);
        }
        fs::write(
            root.join("region").join(format!("r.{region_index}.0.mca")),
            region,
        )
        .unwrap();
    }
    (regions, regions * CHUNKS_PER_REGION)
}

fn gz_level_dat() -> Vec<u8> {
    let mut nbt = Vec::new();
    nbt.extend_from_slice(&[0x0A, 0x00, 0x00]);
    nbt.extend_from_slice(&[0x0A, 0x00, 0x04]);
    nbt.extend_from_slice(b"Data");
    nbt.extend_from_slice(&[0x03, 0x00, 0x0B]);
    nbt.extend_from_slice(b"DataVersion");
    nbt.extend_from_slice(&3955i32.to_be_bytes());
    nbt.extend_from_slice(&[0x0A, 0x00, 0x07]);
    nbt.extend_from_slice(b"Version");
    nbt.extend_from_slice(&[0x08, 0x00, 0x04]);
    nbt.extend_from_slice(b"Name");
    nbt.extend_from_slice(&[0x00, 0x04]);
    nbt.extend_from_slice(b"1.21");
    nbt.push(0x00);
    nbt.push(0x00);
    nbt.push(0x00);
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&nbt).unwrap();
    encoder.finish().unwrap()
}

fn throughput(label: &str, size: usize, elapsed: std::time::Duration) {
    let seconds = elapsed.as_secs_f64();
    let speed = size as f64 / MIB as f64 / seconds;
    println!("  {label:<28} {seconds:8.2}s   {speed:8.1} MiB/s");
}

fn main() {
    let root = tempfile::tempdir().unwrap();
    let sink = make_sink(root.path());
    println!("== NeteaseWorldConverter 流水线基准（release，本机） ==\n");

    for size_mib in SIZES_MIB {
        let size = mb(size_mib);
        println!("--- 输入规模 {size_mib} MiB ---");

        // 1) ZIP 打包（Deflate 等级 1）
        let content = root.path().join(format!("content-{size_mib}"));
        build_content_dir(&content, size, 64);
        let zip_path = root.path().join(format!("bench-{size_mib}.zip"));
        let start = Instant::now();
        create_zip(&content, &zip_path, "World", &sink).unwrap();
        throughput("create_zip（打包）", size, start.elapsed());

        // 2) ZIP 解压（含安全限制扫描）
        let extracted = root.path().join(format!("extracted-{size_mib}"));
        let start = Instant::now();
        let (files, bytes) = extract_zip(&zip_path, &extracted, &sink).unwrap();
        throughput(
            &format!("extract_zip（解压，{files} 文件）"),
            bytes as usize,
            start.elapsed(),
        );

        // 3) 网易 XOR 解密（16 个 .ldb 并行）
        let world = root.path().join(format!("netease-{size_mib}"));
        build_encrypted_world(&world, size);
        let world_info = WorldInfo {
            root: world.clone(),
            database_directory: Some(world.join("db")),
            world_type: WorldType::NeteaseBedrock,
            detected_version: "bench".into(),
            world_name: "bench".into(),
            file_count: 0,
            byte_count: 0,
            notes: vec![],
        };
        let decrypted = root.path().join(format!("bedrock-{size_mib}"));
        let start = Instant::now();
        decrypt::prepare(&world_info, &decrypted, &sink).unwrap();
        throughput("decrypt（XOR 解密+规范化）", size, start.elapsed());

        // 4) Anvil 逐区域验证（zlib 载荷，rayon 并行）
        let anvil = root.path().join(format!("anvil-{size_mib}"));
        let (regions, chunks) = build_anvil_world(&anvil, size);
        let start = Instant::now();
        let result = validate::validate(&anvil, &sink).unwrap();
        throughput("validate（结构验证）", size, start.elapsed());
        println!(
            "    （{} 个区域 / {} chunk，region_files={} region_chunks={}）",
            regions, chunks, result.region_files, result.region_chunks
        );

        // 清理本轮产物，避免占满临时盘
        fs::remove_dir_all(&content).unwrap();
        fs::remove_file(&zip_path).unwrap();
        fs::remove_dir_all(&extracted).unwrap();
        fs::remove_dir_all(&world).unwrap();
        fs::remove_dir_all(&decrypted).unwrap();
        fs::remove_dir_all(&anvil).unwrap();
        println!();
    }
}
