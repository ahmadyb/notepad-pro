#!/usr/bin/env python3
"""Give ui/assets/logo.png a real alpha channel and rebuild app.ico.

The generated logo was RGB-only, so the area outside its rounded rectangle
rendered as four white corners. This re-masks everything connected to the
image border through near-white pixels (the outer background) as transparent,
softens the anti-aliased halo, re-encodes an RGBA PNG, and rebuilds app.ico
from the masked artwork at the usual icon sizes.

Stdlib only (struct + zlib). Run from anywhere:
    python3 tools/fix_logo_alpha.py
"""

import struct
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PNG = ROOT / "ui" / "assets" / "logo.png"
ICO = ROOT / "ui" / "assets" / "app.ico"


def read_png(path):
    """Return (width, height, rows) with rows = list of (r, g, b) per pixel."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        sys.exit("not a png")
    pos = 8
    idat = b""
    width = height = 0
    bitdepth = colortype = interlace = 0
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos:pos + 4])
        typ = data[pos + 4:pos + 8]
        chunk = data[pos + 8:pos + 8 + length]
        if typ == b"IHDR":
            width, height, bitdepth, colortype, _, _, interlace = struct.unpack(
                ">IIBBBBB", chunk)
        elif typ == b"IDAT":
            idat += chunk
        elif typ == b"IEND":
            break
        pos += 12 + length
    if bitdepth != 8 or interlace != 0 or colortype not in (2, 6):
        sys.exit(f"unsupported png (depth {bitdepth}, type {colortype})")
    channels = 4 if colortype == 6 else 3
    raw = zlib.decompress(idat)
    stride = width * channels
    out = bytearray(width * height * channels)
    prev = bytearray(stride)
    p = 0
    for y in range(height):
        filt = raw[p]
        p += 1
        line = bytearray(raw[p:p + stride])
        p += stride
        if filt == 1:  # Sub
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 255
        elif filt == 2:  # Up
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 255
        elif filt == 3:  # Average
            for i in range(stride):
                a = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((a + prev[i]) >> 1)) & 255
        elif filt == 4:  # Paeth
            for i in range(stride):
                a = line[i - channels] if i >= channels else 0
                b = prev[i]
                c = prev[i - channels] if i >= channels else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 255
        out[y * stride:(y + 1) * stride] = line
        prev = line
    return width, height, channels, bytes(out)


def near_white(px):
    return px[0] > 236 and px[1] > 236 and px[2] > 236


def main():
    width, height, channels, pix = read_png(PNG)
    print(f"decoded {width}x{height} channels={channels}")
    stride = width * channels

    def rgb(x, y):
        i = (y * width + x) * channels
        return pix[i], pix[i + 1], pix[i + 2]

    # Flood-fill from the border through near-white pixels: that region is the
    # background the image generator left opaque.
    transparent = bytearray(width * height)
    stack = []
    for x in range(width):
        for y in (0, height - 1):
            if near_white(rgb(x, y)) and not transparent[y * width + x]:
                transparent[y * width + x] = 1
                stack.append((x, y))
    for y in range(height):
        for x in (0, width - 1):
            if near_white(rgb(x, y)) and not transparent[y * width + x]:
                transparent[y * width + x] = 1
                stack.append((x, y))
    while stack:
        x, y = stack.pop()
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            nx, ny = x + dx, y + dy
            if 0 <= nx < width and 0 <= ny < height:
                i = ny * width + nx
                if not transparent[i] and near_white(rgb(nx, ny)):
                    transparent[i] = 1
                    stack.append((nx, ny))

    # Distance (up to 3) from the transparent region, to soften the halo.
    dist = bytearray(b"\xff") * (width * height)
    frontier = [(x, y) for y in range(height) for x in range(width)
                if transparent[y * width + x]]
    for d in range(3):
        nxt = []
        for x, y in frontier:
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nx, ny = x + dx, y + dy
                if 0 <= nx < width and 0 <= ny < height:
                    i = ny * width + nx
                    if not transparent[i] and dist[i] > d + 1:
                        dist[i] = d + 1
                        nxt.append((nx, ny))
        frontier = nxt

    alpha = bytearray(width * height)
    for i in range(width * height):
        if transparent[i]:
            alpha[i] = 0
        else:
            whiteness = (rgb(i % width, i // width)[0] +
                         rgb(i % width, i // width)[1] +
                         rgb(i % width, i // width)[2]) / (3 * 255)
            if dist[i] <= 3 and whiteness > 0.5:
                alpha[i] = int((1.0 - whiteness) * 255)
            else:
                alpha[i] = 255

    print(f"transparent pixels: {sum(1 for a in alpha if a == 0)}")

    # ── re-encode RGBA PNG ────────────────────────────────────────────────
    raw = bytearray()
    for y in range(height):
        raw.append(0)
        for x in range(width):
            i = (y * width + x) * channels
            j = y * width + x
            raw += pix[i:i + 3] + bytes([alpha[j]])
    comp = zlib.compress(bytes(raw), 9)

    def chunk(typ, body):
        c = struct.pack(">I", len(body)) + typ + body
        return c + struct.pack(">I", zlib.crc32(typ + body) & 0xffffffff)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", comp)
    png += chunk(b"IEND", b"")
    PNG.write_bytes(png)
    print(f"wrote {PNG} ({len(png)} bytes, RGBA)")

    # ── downscale + build ICO ─────────────────────────────────────────────
    def box(size):
        out = bytearray(size * size * 4)
        scale = width / size
        for ty in range(size):
            for tx in range(size):
                x0 = int(tx * scale)
                x1 = max(x0 + 1, int((tx + 1) * scale))
                y0 = int(ty * scale)
                y1 = max(y0 + 1, int((ty + 1) * scale))
                r = g = b = a = n = 0
                for sy in range(y0, y1):
                    base = sy * width
                    for sx in range(x0, x1):
                        i = (base + sx) * channels
                        j = base + sx
                        r += pix[i]
                        g += pix[i + 1]
                        b += pix[i + 2]
                        a += alpha[j]
                        n += 1
                o = (ty * size + tx) * 4
                out[o] = r // n
                out[o + 1] = g // n
                out[o + 2] = b // n
                out[o + 3] = a // n
        return bytes(out)

    def small_png(rgba, size):
        raw = bytearray()
        for y in range(size):
            raw.append(0)
            raw += rgba[y * size * 4:(y + 1) * size * 4]
        comp = zlib.compress(bytes(raw), 9)
        p = b"\x89PNG\r\n\x1a\n"
        p += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        p += chunk(b"IDAT", comp)
        p += chunk(b"IEND", b"")
        return p

    sizes = [16, 24, 32, 48, 64, 128, 256]
    entries = []
    offset = 6 + 16 * len(sizes)
    header = struct.pack("<HHH", 0, 1, len(sizes))
    blobs = []
    for s in sizes:
        rgba = box(s)
        data = small_png(rgba, s)
        entries.append(struct.pack("<BBBBHHII",
                                   s & 255, s & 255, 0, 0, 1, 32, len(data), offset))
        blobs.append(data)
        offset += len(data)
    ico = header + b"".join(entries) + b"".join(blobs)
    ICO.write_bytes(ico)
    print(f"wrote {ICO} ({len(ico)} bytes, {sizes})")


if __name__ == "__main__":
    main()
