// Generates the macOS/Windows menu-bar template icons from geometry, so the
// tray art is reproducible and reviewable instead of an opaque binary.
//
// A template image carries shape in the alpha channel only: RGB is pure black
// and the OS recolours it for the light or dark menu bar. Anything else (tinted
// RGB, heavy ink coverage, wrong pixel size) renders as a smudge at 16pt.
//
// Run: node scripts/tray-icons.mjs
import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = resolve(dirname(fileURLToPath(import.meta.url)), "../assets/tray");
// 16 = Windows 1x, 32 = Windows 2x and the popover brand mask,
// 36 = exactly 2x of the 18pt height macOS forces on menu-bar images.
const SIZES = [16, 32, 36];
const SS = 8; // supersampling factor per axis

// Geometry in a unit box. Pointy-top hexagon matches the app icon silhouette.
const HEX_R = 0.4;
const STROKE = 0.052; // half-width; ~1.7px of a 16px glyph
const SEAM = 0.28; // half-height of the centre seam
const BADGE = { x: 0.76, y: 0.76, r: 0.15, gap: 0.055 };

const hexPoints = () =>
  Array.from({ length: 6 }, (_, i) => {
    const angle = (Math.PI / 180) * (90 + i * 60);
    return [0.5 + HEX_R * Math.cos(angle), 0.5 - HEX_R * Math.sin(angle)];
  });

function distanceToSegment(px, py, [ax, ay], [bx, by]) {
  const dx = bx - ax;
  const dy = by - ay;
  const lengthSq = dx * dx + dy * dy;
  const t = lengthSq === 0 ? 0 : Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / lengthSq));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

function distanceToPolygon(px, py, points) {
  let best = Infinity;
  for (let i = 0; i < points.length; i += 1) {
    best = Math.min(best, distanceToSegment(px, py, points[i], points[(i + 1) % points.length]));
  }
  return best;
}

// Each variant answers "is this point ink?" for a supersample.
//
// The three states differ by silhouette, not by fine detail, because a badge
// thinner than a point disappears in a 16pt menu bar: outline, outline plus a
// dot, and solid. Colour cannot carry the difference — a template has none.
function inkTest(variant) {
  const hex = hexPoints();
  const inside = (x, y) => pointInPolygon(x, y, hex);
  return (x, y) => {
    if (variant === "degraded") {
      const d = Math.hypot(x - BADGE.x, y - BADGE.y);
      if (d <= BADGE.r - BADGE.gap) return true; // filled dot
      if (d <= BADGE.r + BADGE.gap) return false; // knockout keeps the dot legible
    }
    if (variant === "unavailable") {
      // Solid body with the seam knocked out: unmistakable at any size.
      if (Math.abs(x - 0.5) <= STROKE && Math.abs(y - 0.5) <= SEAM) return false;
      return inside(x, y);
    }
    if (distanceToPolygon(x, y, hex) <= STROKE) return true;
    if (Math.abs(x - 0.5) <= STROKE && Math.abs(y - 0.5) <= SEAM) return true;
    return false;
  };
}

function pointInPolygon(x, y, points) {
  let hit = false;
  for (let i = 0, j = points.length - 1; i < points.length; j = i, i += 1) {
    const [xi, yi] = points[i];
    const [xj, yj] = points[j];
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) hit = !hit;
  }
  return hit;
}

function rasterize(size, variant) {
  const ink = inkTest(variant);
  const rgba = Buffer.alloc(size * size * 4);
  const step = 1 / (size * SS);
  for (let py = 0; py < size; py += 1) {
    for (let px = 0; px < size; px += 1) {
      let hits = 0;
      for (let sy = 0; sy < SS; sy += 1) {
        for (let sx = 0; sx < SS; sx += 1) {
          const x = (px * SS + sx + 0.5) * step;
          const y = (py * SS + sy + 0.5) * step;
          if (ink(x, y)) hits += 1;
        }
      }
      // Pure black; coverage lives entirely in alpha, as a template requires.
      rgba[(py * size + px) * 4 + 3] = Math.round((hits / (SS * SS)) * 255);
    }
  }
  return rgba;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body) >>> 0);
  return Buffer.concat([length, body, crc]);
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const byte of buf) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function encodePng(size, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y += 1) {
    raw[y * (size * 4 + 1)] = 0; // filter: none
    rgba.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

export const VARIANTS = {
  available: "membrane-template",
  degraded: "membrane-degraded-template",
  unavailable: "membrane-unavailable-template",
};

export function generate() {
  mkdirSync(OUT, { recursive: true });
  const written = [];
  for (const [variant, base] of Object.entries(VARIANTS)) {
    for (const size of SIZES) {
      // Named by pixel size: "@2x" is ambiguous once macOS and Windows want
      // different pixel counts for the same nominal slot.
      const path = resolve(OUT, `${base}-${size}.png`);
      writeFileSync(path, encodePng(size, rasterize(size, variant)));
      written.push(path);
    }
  }
  return written;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  for (const path of generate()) console.log(path);
}
