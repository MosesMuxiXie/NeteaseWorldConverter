// pe-imports.mjs — 解析 PE 导入表（无依赖），列出 exe 依赖的 DLL。
import { readFileSync } from "node:fs";

const buf = readFileSync(process.argv[2]);
if (buf.readUInt16LE(0) !== 0x5a4d) throw new Error("not a PE (no MZ)");
const peOff = buf.readUInt32LE(0x3c);
if (buf.readUInt32LE(peOff) !== 0x4550) throw new Error("not a PE (no PE signature)");
const machine = buf.readUInt16LE(peOff + 4);
const nSections = buf.readUInt16LE(peOff + 6);
const optSize = buf.readUInt16LE(peOff + 20);
const optOff = peOff + 24;
const magic = buf.readUInt16LE(optOff);
const dirOff = magic === 0x20b ? optOff + 112 : optOff + 96; // PE32+ : PE32
const importRva = buf.readUInt32LE(dirOff + 8);  // directory entry 1 = imports
const importSize = buf.readUInt32LE(dirOff + 12);

const secOff = optOff + optSize;
const sections = [];
for (let i = 0; i < nSections; i++) {
  const s = secOff + i * 40;
  sections.push({
    name: buf.toString("ascii", s, s + 8).replace(/\0.*$/, ""),
    va: buf.readUInt32LE(s + 12),
    rawSize: buf.readUInt32LE(s + 16),
    rawOff: buf.readUInt32LE(s + 20),
  });
}
function rvaToOff(rva) {
  for (const s of sections) {
    if (rva >= s.va && rva < s.va + Math.max(s.rawSize, 1)) return rva - s.va + s.rawOff;
  }
  return null;
}
console.log(`machine=${machine === 0x8664 ? "x64" : machine === 0x14c ? "x86" : machine} sections=${nSections}`);
if (importRva === 0) { console.log("no import table"); } else {
  const off = rvaToOff(importRva);
  const dlls = [];
  let idx = 0;
  while (true) {
    const desc = off + idx * 20;
    const nameRva = buf.readUInt32LE(desc + 12);
    if (nameRva === 0) break;
    const nameOff = rvaToOff(nameRva);
    const name = buf.toString("ascii", nameOff, buf.indexOf(0, nameOff));
    dlls.push(name);
    idx++;
    if (idx > 256) break;
  }
  console.log("imports:\n" + dlls.join("\n"));
}
// 函数级导入（逐 DLL 打印缺失风险点）
if (importRva !== 0 && process.argv[3] === "--functions") {
  const off = rvaToOff(importRva);
  let idx = 0;
  while (true) {
    const desc = off + idx * 20;
    const nameRva = buf.readUInt32LE(desc + 12);
    if (nameRva === 0) break;
    const nameOff = rvaToOff(nameRva);
    const dllName = buf.toString("ascii", nameOff, buf.indexOf(0, nameOff));
    const iatRva = buf.readUInt32LE(desc + 16);
    const iltRva = buf.readUInt32LE(desc); // 0 => use IAT
    const thunkRva = iltRva || iatRva;
    const thunkOff = rvaToOff(thunkRva);
    const lines = [];
    let t = 0;
    while (true) {
      const val = buf.readBigUInt64LE(thunkOff + t * 8);
      if (val === 0n) break;
      if ((val & (1n << 63n)) !== 0n) {
        lines.push(`  #${Number(val & 0xffffn)} (ordinal)`);
      } else {
        const fnOff = rvaToOff(Number(val & 0x7fffffffn) + 2);
        const fn = buf.toString("ascii", fnOff, buf.indexOf(0, fnOff));
        lines.push(`  ${fn}`);
      }
      t++;
      if (t > 4096) break;
    }
    console.log(`${dllName}:`);
    console.log(lines.join("\n"));
    idx++;
    if (idx > 256) break;
  }
}
// 延迟导入表（数据目录第 13 项）
const delayRva = buf.readUInt32LE(dirOff + 8 + 12 * 8);
if (delayRva) {
  const off = rvaToOff(delayRva);
  const dlls = [];
  let idx = 0;
  while (true) {
    const desc = off + idx * 32;
    const nameRva = buf.readUInt32LE(desc + 4);
    if (nameRva === 0) break;
    const nameOff = rvaToOff(nameRva);
    const name = buf.toString("ascii", nameOff, buf.indexOf(0, nameOff));
    dlls.push(name);
    idx++;
    if (idx > 256) break;
  }
  console.log("delay-imports:\n" + dlls.join("\n"));
} else {
  console.log("no delay-import table");
}
