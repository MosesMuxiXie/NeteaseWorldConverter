// make-test-world.mjs — 生成一个最小的合法 Java 1.21 Anvil 世界 ZIP（用于 E2E 测试）。
// 结构：level.dat（gzip NBT，DataVersion 3955/1.21）+ region/r.0.0.mca（1 个无压缩空 Compound chunk）
import { gzipSync, deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";

const out = process.argv[2] || "e2e-test-world.zip";
const dir = ".e2e-world";

function nbtLevelDat() {
  const nbt = [];
  nbt.push(0x0a, 0x00, 0x00); // root ""
  nbt.push(0x0a, 0x00, 0x04, ...Buffer.from("Data"));
  nbt.push(0x03, 0x00, 0x0b, ...Buffer.from("DataVersion"), 0x00, 0x00, 0x0f, 0x73); // 3955
  nbt.push(0x08, 0x00, 0x09, ...Buffer.from("LevelName"), 0x00, 0x09, ...Buffer.from("E2E测试"));
  nbt.push(0x0a, 0x00, 0x07, ...Buffer.from("Version"));
  nbt.push(0x08, 0x00, 0x04, ...Buffer.from("Name"), 0x00, 0x05, ...Buffer.from("1.21.0"));
  nbt.push(0x00); // end Version
  nbt.push(0x00); // end Data
  nbt.push(0x00); // end root
  return gzipSync(Buffer.from(nbt));
}

function regionFile() {
  const nbt = [0x0a, 0x00, 0x00, 0x00]; // 空 Compound
  const header = Buffer.alloc(8192); // 位置表 1024 项 × 8 字节（扇区 0/1）
  header[0] = 0x00; header[1] = 0x00; header[2] = 0x02; // offset=2
  header[3] = 0x01; // 1 sector
  const data = Buffer.alloc(4096);
  data.writeUInt32BE(nbt.length + 1, 0); // 长度（含压缩字节）
  data[4] = 3; // 无压缩
  Buffer.from(nbt).copy(data, 5);
  return Buffer.concat([header, data]);
}

function makeZip() {
  const files = {
    "level.dat": nbtLevelDat(),
    "region/r.0.0.mca": regionFile(),
  };
  const chunks = [];
  const crcTable = new Int32Array(256).map((_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c;
  });
  const crc32 = (b) => {
    let c = 0xffffffff;
    for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const central = [];
  let offset = 0;
  const enc = (s) => Buffer.from(s, "utf8");
  for (const [name, data] of Object.entries(files)) {
    const nameB = enc(name);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);       // version needed
    local.writeUInt16LE(0x0800, 6);   // flags: UTF-8
    local.writeUInt16LE(8, 8);        // deflate
    local.writeUInt16LE(0, 10);       // time
    local.writeUInt16LE(0x21, 12);    // date
    const compressed = deflateSync(data);
    local.writeUInt32LE(crc32(data), 14);
    local.writeUInt32LE(compressed.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameB.length, 26);
    local.writeUInt16LE(0, 28);
    chunks.push(local, nameB, compressed);
    const cen = Buffer.alloc(46);
    cen.writeUInt32LE(0x02014b50, 0);
    cen.writeUInt16LE(20, 4);
    cen.writeUInt16LE(20, 6);
    cen.writeUInt16LE(0x0800, 8);
    cen.writeUInt16LE(8, 10);
    cen.writeUInt16LE(0, 12);
    cen.writeUInt16LE(0x21, 14);
    cen.writeUInt32LE(crc32(data), 16);
    cen.writeUInt32LE(compressed.length, 20);
    cen.writeUInt32LE(data.length, 24);
    cen.writeUInt16LE(nameB.length, 28);
    cen.writeUInt32LE(offset, 42);
    central.push(cen, nameB);
    offset += local.length + nameB.length + compressed.length;
  }
  const centralSize = central.reduce((s, b) => s + b.length, 0);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(files.length, 8);
  eocd.writeUInt16LE(files.length, 10);
  eocd.writeUInt32LE(centralSize, 12);
  eocd.writeUInt32LE(offset, 16);
  return Buffer.concat([...chunks, ...central, eocd]);
}

rmSync(dir, { recursive: true, force: true });
mkdirSync(`${dir}/region`, { recursive: true });
writeFileSync(`${dir}/level.dat`, nbtLevelDat());
writeFileSync(`${dir}/region/r.0.0.mca`, regionFile());
writeFileSync(out, makeZip());
console.log("test world zip:", out);
