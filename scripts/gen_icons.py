#!/usr/bin/env python3
"""Generate the companion icons without any image library.

Outputs (into src-tauri/icons/):
  - icon.ico            app icon (16/24/32/48 BMP entries + 256 PNG entry)
  - tray_idle.png       32x32 tray glyphs, one per visual state
  - tray_queued.png
  - tray_starting.png
  - tray_ready.png
  - tray_error.png
  - tray_stopped.png    (== idle glyph, neutral)

Glyph language: a TPU "chip" (rounded square with side pins) and one central
die dot. The dot carries the semantic state colour; the outline stays neutral
so the same silhouette reads in light and dark system trays.
"""
import io
import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
OUT.mkdir(parents=True, exist_ok=True)

OUTLINE = (201, 209, 217)   # C9D1D9 light neutral
APP_BG = (27, 30, 36)       # 1B1E24 off-black
DOT = {
    "idle": (139, 147, 161),    # 8B93A1
    "queued": (232, 179, 61),   # E8B33D
    "starting": (232, 179, 61), # E8B33D
    "ready": (63, 185, 80),     # 3FB950
    "error": (248, 81, 73),     # F85149
    "stopped": (139, 147, 161), # 8B93A1
}


def clamp01(x):
    return 0.0 if x < 0 else (1.0 if x > 1 else x)


def sd_rounded_rect(px, py, cx, cy, hw, hh, r):
    qx = abs(px - cx) - (hw - r)
    qy = abs(py - cy) - (hh - r)
    ox = max(qx, 0.0)
    oy = max(qy, 0.0)
    return (ox * ox + oy * oy) ** 0.5 + min(max(qx, qy), 0.0) - r


def sd_circle(px, py, cx, cy, r):
    return ((px - cx) ** 2 + (py - cy) ** 2) ** 0.5 - r


def sd_box(px, py, cx, cy, hw, hh):
    dx = max(abs(px - cx) - hw, 0.0)
    dy = max(abs(py - cy) - hh, 0.0)
    return (dx * dx + dy * dy) ** 0.5


def smooth(a, b, x):
    t = clamp01((x - a) / (b - a))
    return t * t * (3 - 2 * t)


def render(size, dot_color, with_bg=False):
    """Render the glyph at `size` with 4x supersampling. Returns RGBA bytes."""
    ss = 4
    S = size * ss
    acc = [[0.0, 0.0, 0.0, 0.0] for _ in range(S * S)]
    cx = cy = S / 2.0
    hw = hh = S * 0.315
    r = S * 0.14
    thick = S * 0.075
    dot_r = S * 0.115
    pin_w = S * 0.085
    pin_h = S * 0.05
    pin_gap = S * 0.21
    for y in range(S):
        for x in range(S):
            X = x + 0.5
            Y = y + 0.5
            bg_a = 0.0
            if with_bg:
                d_bg = sd_rounded_rect(X, Y, cx, cy, S * 0.47, S * 0.47, S * 0.16)
                bg_a = 1.0 - smooth(-0.5, 0.5, d_bg)
            d = sd_rounded_rect(X, Y, cx, cy, hw, hh, r)
            a_outline = 1.0 - smooth(thick / 2 - 0.5, thick / 2 + 0.5, abs(d))
            for side in (-1, 1):
                for i in (-1, 0, 1):
                    pyy = cy + i * pin_gap
                    pxc = cx + side * (hw + pin_w * 0.55)
                    dp = sd_box(X, Y, pxc, pyy, pin_w / 2, pin_h / 2)
                    a_pin = 1.0 - smooth(-0.5, 0.5, dp)
                    if a_pin > a_outline:
                        a_outline = a_pin
            dd = sd_circle(X, Y, cx, cy, dot_r)
            a_dot = 1.0 - smooth(-0.5, 0.5, dd)
            # premultiplied composite: dot over outline over bg
            r_c = dot_color[0] * a_dot
            g_c = dot_color[1] * a_dot
            b_c = dot_color[2] * a_dot
            a_c = a_dot
            o = (1.0 - a_c) * a_outline
            r_c += OUTLINE[0] * o
            g_c += OUTLINE[1] * o
            b_c += OUTLINE[2] * o
            a_c += o
            if with_bg:
                b_ = (1.0 - a_c) * bg_a
                r_c += APP_BG[0] * b_
                g_c += APP_BG[1] * b_
                b_c += APP_BG[2] * b_
                a_c += b_
            acc[y * S + x] = [r_c, g_c, b_c, a_c]
    out = bytearray()
    n = ss * ss
    for y in range(size):
        out.append(0)  # PNG filter: none
        for x in range(size):
            r = g = b = a = 0.0
            for sy in range(ss):
                for sx in range(ss):
                    p = acc[(y * ss + sy) * S + (x * ss + sx)]
                    r += p[0]
                    g += p[1]
                    b += p[2]
                    a += p[3]
            # `acc` stores premultiplied RGB with alpha in 0..1. PNG RGBA
            # expects straight RGB and an 8-bit alpha channel (0..255).
            alpha = a / n
            if alpha > 0.0:
                rr = (r / n) / alpha
                gg = (g / n) / alpha
                bb = (b / n) / alpha
            else:
                rr = gg = bb = 0.0
            out += bytes((
                int(max(0.0, min(255.0, rr)) + 0.5) & 0xFF,
                int(max(0.0, min(255.0, gg)) + 0.5) & 0xFF,
                int(max(0.0, min(255.0, bb)) + 0.5) & 0xFF,
                int(max(0.0, min(1.0, alpha)) * 255.0 + 0.5) & 0xFF,
            ))
    return bytes(out)


def png_chunk(tag, data):
    c = struct.pack(">I", len(data)) + tag + data
    c += struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    return c


def png_bytes(size, rgba):
    buf = io.BytesIO()
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    idat = zlib.compress(rgba, 9)
    buf.write(b"\x89PNG\r\n\x1a\n")
    buf.write(png_chunk(b"IHDR", ihdr))
    buf.write(png_chunk(b"IDAT", idat))
    buf.write(png_chunk(b"IEND", b""))
    return buf.getvalue()


def write_png(path, size, rgba):
    path.write_bytes(png_bytes(size, rgba))


def bmp_entry(size, rgba):
    """BMP (BGRA) + AND mask for an ICO entry. rgba is top-down RGBA bytes."""
    w = h = size
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    px = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            i = (y * w + x) * 4
            b, g, r, a = rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]
            px += bytes((b, g, r, a))
    mask = bytearray()
    for y in range(h - 1, -1, -1):
        row = bytearray()
        for x in range(w):
            a = rgba[(y * w + x) * 4 + 3]
            row.append(0xFF if a < 128 else 0x00)
        pad = (32 - (len(row) * 8) % 32) % 32
        row += b"\x00" * (pad // 8)
        mask += row
    return header + bytes(px) + bytes(mask)


def write_ico(path, entries):
    """entries: list of (size, rgba_bytes). size==256 is stored as PNG."""
    n = len(entries)
    offset = 6 + 16 * n
    dirs = []
    blobs = []
    for size, rgba in entries:
        blob = png_bytes(size, rgba) if size == 256 else bmp_entry(size, rgba)
        dirs.append(struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(blob), offset))
        offset += len(blob)
        blobs.append(blob)
    with open(path, "wb") as f:
        f.write(struct.pack("<HHH", 0, 1, n))
        for d in dirs:
            f.write(d)
        for b in blobs:
            f.write(b)


def main():
    for name in ("idle", "queued", "starting", "ready", "error", "stopped"):
        rgba = render(32, DOT[name], with_bg=False)
        write_png(OUT / f"tray_{name}.png", 32, rgba)
        print("wrote", OUT / f"tray_{name}.png")
    entries = []
    for size in (16, 24, 32, 48, 256):
        rgba = render(size, DOT["ready"], with_bg=True)
        entries.append((size, rgba))
    write_ico(OUT / "icon.ico", entries)
    print("wrote", OUT / "icon.ico")


if __name__ == "__main__":
    main()
