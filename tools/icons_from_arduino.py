#!/usr/bin/env python3
"""Turns the 48 x 48 1-bit C arrays of the Arduino `OpenMeteo-Weather` sketch into 1-bit BMP files.

    python3 tools/icons_from_arduino.py ~/Src/Arduino/LilyGoT5ePaper/OpenMeteo-Weather/icons assets/icons

Each header is `const uint8_t icon_s_<name>[] PROGMEM = { 0x.., ... };`, 6 bytes per row, 48 rows, MSB first,
a set bit is ink. In the BMP an ink pixel is black (palette entry 0) and the rest is white (entry 1). The
script reads every BMP back and compares it with the source bits, so a wrong layout fails here.
The icons are Lucide icons (ISC licence), see `assets/NOTICE.md`. Standard library only.
"""
import re
import struct
import sys
from pathlib import Path

W = H = 48
STRIDE = W // 8


def read_array(path: Path) -> bytes:
    text = path.read_text()
    body = text[text.index("{") + 1 : text.rindex("}")]
    data = bytes(int(v, 16) for v in re.findall(r"0x[0-9a-fA-F]+", body))
    if len(data) != STRIDE * H:
        raise SystemExit(f"{path}: {len(data)} bytes, expected {STRIDE * H}")
    return data


def bmp_bytes(bits: bytes) -> bytes:
    """1 bpp BMP, bottom-up rows padded to 4 bytes, palette black then white. Ink is index 0."""
    row_size = (STRIDE + 3) // 4 * 4
    pixels = bytearray()
    for y in range(H - 1, -1, -1):
        row = bytearray(row_size)
        for x in range(STRIDE):
            row[x] = (~bits[y * STRIDE + x]) & 0xFF  # ink (1 in the source) is palette index 0
        pixels += row
    offset = 14 + 40 + 8
    header = b"BM" + struct.pack("<IHHI", offset + len(pixels), 0, 0, offset)
    dib = struct.pack("<IiiHHIIiiII", 40, W, H, 1, 1, 0, len(pixels), 2835, 2835, 2, 0)
    palette = bytes([0, 0, 0, 0, 255, 255, 255, 0])
    return header + dib + palette + bytes(pixels)


def bits_from_bmp(data: bytes) -> bytes:
    offset = struct.unpack_from("<I", data, 10)[0]
    width, height = struct.unpack_from("<ii", data, 18)
    assert (width, height) == (W, H) and struct.unpack_from("<H", data, 28)[0] == 1
    row_size = (STRIDE + 3) // 4 * 4
    out = bytearray()
    for y in range(H - 1, -1, -1):
        row = data[offset + y * row_size : offset + y * row_size + STRIDE]
        out += bytes((~b) & 0xFF for b in row)
    return bytes(out)


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    src, dst = Path(sys.argv[1]).expanduser(), Path(sys.argv[2])
    dst.mkdir(parents=True, exist_ok=True)
    for header in sorted(src.glob("icon_s_*.h")):
        name = header.stem.removeprefix("icon_s_")
        bits = read_array(header)
        data = bmp_bytes(bits)
        assert bits_from_bmp(data) == bits, name
        (dst / f"{name}.bmp").write_bytes(data)
        ink = sum(bin(b).count("1") for b in bits)
        print(f"{name}.bmp  {len(data)} bytes  {ink} ink pixels")


if __name__ == "__main__":
    main()
