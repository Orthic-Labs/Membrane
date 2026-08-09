#!/usr/bin/env node
// Generates the menu-bar tray icons from the Membrane hex-brain artwork.
//
// The artwork is NOT drawn here. membrane-source@2x.png is the master and the
// only source of shape; this script reads its alpha channel, crops away the
// transparent margin, resamples, and tints. Status variants differ by colour
// alone -- same glyph every time.
//
// Two things this encodes that are easy to get wrong:
//
//   * Cropping to the alpha bounding box is what makes the icon look the same
//     size as its menu-bar neighbours. tray-icon scales the whole canvas to
//     18pt tall (icon_height in its macOS backend), so any transparent padding
//     is spent inside that 18pt: the master's glyph covers 77% of its canvas
//     height, which rendered as a ~14pt mark next to everyone else's 18pt.
//
//   * These are NOT template images. macOS ignores RGB in a template and tints
//     the alpha itself, which would throw the status colour away. main.rs must
//     set icon_as_template(false) for these to render in colour.
//
// Run: node scripts/tray-icons.mjs
import { createHash } from "node:crypto";
import { deflateSync, inflateSync } from "node:zlib";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const TRAY = join(HERE, "..", "assets", "tray");
const SOURCE = join(TRAY, "membrane-source@2x.png");

// Straight from src/popover.css -- the tray must not invent a second palette.
const VARIANTS = {
  available: "#39ff72",
  degraded: "#ffc857",
  unavailable: "#ff6b6b",
  offline: "#aeb7c2",
};

// The menu bar is 18pt tall. 36 = 2x, the native retina size, so the glyph
// lands on whole pixels instead of being resampled by Cocoa. 128 is for the
// popover brand mark, which masks the same file at 28 CSS px.
const TRAY_HEIGHTS = { 36: "macOS 18pt at 2x", 32: "Windows notification area" };
const MARK_HEIGHT = 128;

function chunks(buffer) {
  const found = [];
  let offset = 8;
  while (offset < buffer.length) {
    const length = buffer.readUInt32BE(offset);
    found.push({ type: buffer.toString("ascii", offset + 4, offset + 8), data: buffer.subarray(offset + 8, offset + 8 + length) });
    offset += 12 + length;
  }
  return found;
}

function decode(path) {
  const parts = chunks(readFileSync(path));
  const header = parts.find((part) => part.type === "IHDR").data;
  const width = header.readUInt32BE(0);
  const height = header.readUInt32BE(4);
  if (header[8] !== 8 || header[9] !== 6) throw new Error(`${path}: expected 8-bit RGBA`);
  const raw = inflateSync(Buffer.concat(parts.filter((part) => part.type === "IDAT").map((part) => part.data)));
  const stride = width * 4;
  const pixels = Buffer.alloc(height * stride);
  let offset = 0;
  for (let y = 0; y < height; y += 1) {
    const filter = raw[offset];
    offset += 1;
    const line = pixels.subarray(y * stride, (y + 1) * stride);
    raw.copy(line, 0, offset, offset + stride);
    offset += stride;
    const above = y > 0 ? pixels.subarray((y - 1) * stride, y * stride) : null;
    for (let x = 0; x < stride; x += 1) {
      const a = x >= 4 ? line[x - 4] : 0;
      const b = above ? above[x] : 0;
      const c = above && x >= 4 ? above[x - 4] : 0;
      if (filter === 1) line[x] = (line[x] + a) & 255;
      else if (filter === 2) line[x] = (line[x] + b) & 255;
      else if (filter === 3) line[x] = (line[x] + ((a + b) >> 1)) & 255;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a); const pb = Math.abs(p - b); const pc = Math.abs(p - c);
        line[x] = (line[x] + (pa <= pb && pa <= pc ? a : pb <= pc ? b : c)) & 255;
      }
    }
  }
  return { width, height, pixels };
}

function encode(width, height, pixels) {
  const stride = width * 4;
  const raw = Buffer.alloc(height * (stride + 1));
  for (let y = 0; y < height; y += 1) {
    raw[y * (stride + 1)] = 0;
    pixels.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  const chunk = (type, data) => {
    const out = Buffer.alloc(data.length + 12);
    out.writeUInt32BE(data.length, 0);
    out.write(type, 4, "ascii");
    data.copy(out, 8);
    out.writeInt32BE(crc(Buffer.concat([Buffer.from(type, "ascii"), data])), data.length + 8);
    return out;
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8; header[9] = 6;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

const CRC_TABLE = Array.from({ length: 256 }, (_, index) => {
  let value = index;
  for (let bit = 0; bit < 8; bit += 1) value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  return value >>> 0;
});
function crc(buffer) {
  let value = 0xffffffff;
  for (const byte of buffer) value = CRC_TABLE[(value ^ byte) & 0xff] ^ (value >>> 8);
  return (value ^ 0xffffffff) | 0;
}

// Tightest rectangle holding every non-transparent pixel.
function alphaBounds({ width, height, pixels }) {
  let minX = width; let minY = height; let maxX = -1; let maxY = -1;
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      if (pixels[(y * width + x) * 4 + 3] <= 8) continue;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
  }
  if (maxX < 0) throw new Error("source artwork is fully transparent");
  return { minX, minY, width: maxX - minX + 1, height: maxY - minY + 1 };
}

// Area-average resample of the cropped alpha. Only alpha is carried over: the
// master's RGB is near-white and would fight the tint.
function resampleAlpha(source, bounds, outWidth, outHeight) {
  const out = new Float64Array(outWidth * outHeight);
  const scaleX = bounds.width / outWidth;
  const scaleY = bounds.height / outHeight;
  for (let y = 0; y < outHeight; y += 1) {
    const y0 = bounds.minY + y * scaleY;
    const y1 = y0 + scaleY;
    for (let x = 0; x < outWidth; x += 1) {
      const x0 = bounds.minX + x * scaleX;
      const x1 = x0 + scaleX;
      let total = 0; let weight = 0;
      for (let sy = Math.floor(y0); sy < Math.ceil(y1); sy += 1) {
        const wy = Math.min(y1, sy + 1) - Math.max(y0, sy);
        if (wy <= 0) continue;
        for (let sx = Math.floor(x0); sx < Math.ceil(x1); sx += 1) {
          const wx = Math.min(x1, sx + 1) - Math.max(x0, sx);
          if (wx <= 0) continue;
          total += source.pixels[(sy * source.width + sx) * 4 + 3] * wx * wy;
          weight += wx * wy;
        }
      }
      out[y * outWidth + x] = weight > 0 ? total / weight : 0;
    }
  }
  return out;
}

function tint(alpha, width, height, hex) {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  const pixels = Buffer.alloc(width * height * 4);
  for (let index = 0; index < width * height; index += 1) {
    pixels[index * 4] = r;
    pixels[index * 4 + 1] = g;
    pixels[index * 4 + 2] = b;
    pixels[index * 4 + 3] = Math.round(Math.max(0, Math.min(255, alpha[index])));
  }
  return pixels;
}

function emit(source, bounds, height, hex, name) {
  const width = Math.max(1, Math.round((bounds.width / bounds.height) * height));
  const alpha = resampleAlpha(source, bounds, width, height);
  const path = join(TRAY, name);
  writeFileSync(path, encode(width, height, tint(alpha, width, height, hex)));
  return { name, width, height };
}

const source = decode(SOURCE);
const bounds = alphaBounds(source);
console.log(`source ${source.width}x${source.height}, glyph bounds ${bounds.width}x${bounds.height} at (${bounds.minX},${bounds.minY})`);
console.log(`cropped away ${(100 - (100 * bounds.height) / source.height).toFixed(0)}% of canvas height`);

const written = [];
for (const [status, hex] of Object.entries(VARIANTS)) {
  for (const height of Object.keys(TRAY_HEIGHTS)) {
    written.push(emit(source, bounds, Number(height), hex, `membrane-${status}-${height}.png`));
  }
}
written.push(emit(source, bounds, MARK_HEIGHT, "#ffffff", "membrane-mark-128.png"));

for (const item of written) {
  const digest = createHash("sha256").update(readFileSync(join(TRAY, item.name))).digest("hex").slice(0, 12);
  console.log(`${item.name.padEnd(30)} ${item.width}x${item.height}  sha256:${digest}`);
}
