// make-icon.mjs — 纯 Node 生成 1024x1024 应用图标 PNG（无依赖）。
// 画面：深色圆角底 + 等距草方块（草绿顶面 / 棕色侧面）。
// 用法：node scripts/make-icon.mjs src-tauri/icons/app-icon.png

import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";

const SIZE = 1024;
const px = new Uint8Array(SIZE * SIZE * 4);

function set(x, y, r, g, b, a = 255) {
  const i = (y * SIZE + x) * 4;
  px[i] = r; px[i + 1] = g; px[i + 2] = b; px[i + 3] = a;
}

function inPoly(x, y, pts) {
  let inside = false;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const [xi, yi] = pts[i], [xj, yj] = pts[j];
    if ((yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

function bbox(pts) {
  return {
    x0: Math.floor(Math.min(...pts.map(p => p[0]))),
    y0: Math.floor(Math.min(...pts.map(p => p[1]))),
    x1: Math.ceil(Math.max(...pts.map(p => p[0]))),
    y1: Math.ceil(Math.max(...pts.map(p => p[1]))),
  };
}

function fillPoly(pts, color, checker = null) {
  const { x0, y0, x1, y1 } = bbox(pts);
  for (let y = y0; y <= y1; y++) {
    for (let x = x0; x <= x1; x++) {
      if (x < 0 || y < 0 || x >= SIZE || y >= SIZE) continue;
      if (!inPoly(x + 0.5, y + 0.5, pts)) continue;
      if (checker) {
        const cell = 32;
        const on = (Math.floor((x - 200) / cell) + Math.floor((y - 200) / cell)) % 2 === 0;
        set(x, y, ...(on ? checker[0] : checker[1]));
      } else {
        set(x, y, ...color);
      }
    }
  }
}

// 背景：深色圆角方（超采样边缘 4x）
const R = 190;
for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    let inside = 0;
    for (let sy = 0; sy < 4; sy++) {
      for (let sx = 0; sx < 4; sx++) {
        const fx = x + (sx + 0.5) / 4, fy = y + (sy + 0.5) / 4;
        const dx = Math.max(Math.abs(fx - SIZE / 2) - (SIZE / 2 - R), 0);
        const dy = Math.max(Math.abs(fy - SIZE / 2) - (SIZE / 2 - R), 0);
        if (dx * dx + dy * dy <= R * R) inside++;
      }
    }
    if (inside === 0) continue;
    // 垂直渐变 #2f3542 → #232836
    const t = y / SIZE;
    const r = Math.round(0x2f + (0x23 - 0x2f) * t);
    const g = Math.round(0x35 + (0x28 - 0x35) * t);
    const b = Math.round(0x42 + (0x36 - 0x42) * t);
    set(x, y, r, g, b, Math.round(255 * inside / 16));
  }
}

// 等距草方块
const top = [[512, 190], [816, 352], [512, 514], [208, 352]];
const left = [[208, 352], [512, 514], [512, 726], [208, 564]];
const right = [[512, 514], [816, 352], [816, 564], [512, 726]];
fillPoly(left, [0x8d, 0x6e, 0x63]);   // 左侧面 浅棕
fillPoly(right, [0x5d, 0x40, 0x37]);  // 右侧面 深棕
fillPoly(top, [0x7c, 0xb3, 0x42], [[0x7c, 0xb3, 0x42, 255], [0x8b, 0xc3, 0x4d, 255]]);

// 底边阴影
for (let y = 726; y <= 780; y++) {
  for (let x = 120; x <= 904; x++) {
    if (!inPoly(x + 0.5, y + 0.5, [[180, 700], [844, 700], [844, 760], [180, 760]])) continue;
    const dist = y - 726;
    if (dist >= 0 && dist < 54) {
      const a = Math.round(60 * (1 - dist / 54));
      set(x, y, 0, 0, 0, Math.max(a, 0));
    }
  }
}

// ---------- PNG 编码 ----------
const CRC_TABLE = new Int32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8;  // bit depth
ihdr[9] = 6;  // RGBA
const raw = Buffer.alloc((SIZE * 4 + 1) * SIZE);
for (let y = 0; y < SIZE; y++) {
  raw[y * (SIZE * 4 + 1)] = 0; // filter none
  px.subarray(y * SIZE * 4, (y + 1) * SIZE * 4).forEach((v, i) => {
    raw[y * (SIZE * 4 + 1) + 1 + i] = v;
  });
}
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);

const out = resolve(process.argv[2] || "src-tauri/icons/app-icon.png");
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, png);
console.log("icon written:", out, png.length, "bytes");
