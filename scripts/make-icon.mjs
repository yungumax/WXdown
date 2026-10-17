// 生成应用图标源图（1024×1024 PNG）：
// 微信主题绿的圆角方块 + 白色下载箭头（托盘）。
// 不引第三方依赖：手写 PNG 编码（zlib + CRC32），3×3 超采样做抗锯齿。
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const SIZE = 1024;
const SS = 3; // 每轴超采样
const R = 232; // 圆角半径

// 品牌色：#07C160 微信绿，顶部略亮
const TOP = [0x2f, 0xd0, 0x7c];
const BOTTOM = [0x06, 0xad, 0x56];

const lerp = (a, b, t) => a + (b - a) * t;
const clamp01 = (v) => Math.min(1, Math.max(0, v));

// 圆角矩形内部判定（SDF <= 0）
function inRoundRect(x, y, x0, y0, x1, y1, r) {
  const cx = Math.min(Math.max(x, x0 + r), x1 - r);
  const cy = Math.min(Math.max(y, y0 + r), y1 - r);
  const dx = x - cx;
  const dy = y - cy;
  return dx * dx + dy * dy <= r * r || (x >= x0 && x <= x1 && y >= cy && y <= y1) || (y >= y0 && y <= y1 && x >= cx && x <= x1);
}

// 胶囊（两端半圆的长条）
function inCapsule(x, y, x0, y0, x1, y1, r) {
  const cx = Math.min(Math.max(x, x0 + r), x1 - r);
  const cy = Math.min(Math.max(y, y0 + r), y1 - r);
  const dx = x - cx;
  const dy = y - cy;
  return dx * dx + dy * dy <= r * r;
}

// 三角形（重心法）
function inTriangle(px, py, ax, ay, bx, by, cx, cy) {
  const d1 = (px - bx) * (ay - by) - (ax - bx) * (py - by);
  const d2 = (px - cx) * (by - cy) - (bx - cx) * (py - cy);
  const d3 = (px - ax) * (cy - ay) - (cx - ax) * (py - ay);
  const hasNeg = d1 < 0 || d2 < 0 || d3 < 0;
  const hasPos = d1 > 0 || d2 > 0 || d3 > 0;
  return !(hasNeg && hasPos);
}

function sample(x, y) {
  // 背景圆角方块
  if (!inRoundRect(x, y, 0, 0, SIZE, SIZE, R)) return null;

  // 白色箭头（竖杆 + 三角头）与底部托盘
  const stem = inCapsule(x, y, 462, 232, 562, 568, 50);
  const head = inTriangle(x, y, 352, 508, 672, 508, 512, 744);
  const tray = inCapsule(x, y, 300, 742, 724, 802, 30);
  if (stem || head || tray) return [255, 255, 255, 255];

  const t = clamp01((x + y) / (2 * SIZE));
  return [
    Math.round(lerp(TOP[0], BOTTOM[0], t)),
    Math.round(lerp(TOP[1], BOTTOM[1], t)),
    Math.round(lerp(TOP[2], BOTTOM[2], t)),
    255,
  ];
}

// 渲染（超采样 → 预乘平均）
const px = Buffer.alloc(SIZE * SIZE * 4);
const step = 1 / SS;
for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    let r = 0, g = 0, b = 0, a = 0;
    for (let sy = 0; sy < SS; sy++) {
      for (let sx = 0; sx < SS; sx++) {
        const s = sample(x + (sx + 0.5) * step, y + (sy + 0.5) * step);
        if (!s) continue;
        const al = s[3] / 255;
        r += s[0] * al;
        g += s[1] * al;
        b += s[2] * al;
        a += al;
      }
    }
    const n = SS * SS;
    const i = (y * SIZE + x) * 4;
    if (a === 0) continue;
    px[i] = Math.round(r / a);
    px[i + 1] = Math.round(g / a);
    px[i + 2] = Math.round(b / a);
    px[i + 3] = Math.round((a / n) * 255);
  }
}

// ---- PNG 编码 ----
const CRC_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();

function crc32(buf) {
  let c = -1;
  for (const byte of buf) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
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
// 10..12 = compression/filter/interlace = 0

const raw = Buffer.alloc(SIZE * (SIZE * 4 + 1));
for (let y = 0; y < SIZE; y++) {
  raw[y * (SIZE * 4 + 1)] = 0; // filter none
  px.copy(raw, y * (SIZE * 4 + 1) + 1, y * SIZE * 4, (y + 1) * SIZE * 4);
}

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);

const out = process.argv[2] || "app-icon.png";
writeFileSync(out, png);
console.log(`已生成 ${out}（${SIZE}×${SIZE}，${(png.length / 1024).toFixed(1)} KiB）`);
