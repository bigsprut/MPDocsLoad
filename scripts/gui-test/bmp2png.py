# -*- coding: utf-8 -*-
"""BMP -> PNG (stdlib only: struct + zlib). 24/32-bit uncompressed BMP."""
import struct, sys, zlib

def bmp_to_png(src_path, dst_path):
    data = open(src_path, 'rb').read()
    off = struct.unpack_from('<I', data, 10)[0]
    hdr_size = struct.unpack_from('<I', data, 14)[0]
    w = struct.unpack_from('<i', data, 18)[0]
    h_raw = struct.unpack_from('<i', data, 22)[0]
    bpp = struct.unpack_from('<H', data, 28)[0]
    comp = struct.unpack_from('<I', data, 30)[0]
    bottom_up = h_raw > 0
    h = abs(h_raw)
    assert comp == 0, f"compressed bmp not supported ({comp})"
    assert bpp in (24, 32), f"bpp={bpp} not supported"
    bytes_pp = bpp // 8
    row_size = (w * bytes_pp + 3) & ~3
    rows = []
    for y in range(h):
        src_y = (h - 1 - y) if bottom_up else y
        row = bytearray()
        base = off + src_y * row_size
        for x in range(w):
            i = base + x * bytes_pp
            b, g, r = data[i], data[i + 1], data[i + 2]
            row += bytes((r, g, b))
        rows.append(bytes(row))
    raw = b''.join(b'\x00' + r for r in rows)  # filter 0 per row

    def chunk(tag, payload):
        c = tag + payload
        return struct.pack('>I', len(payload)) + c + struct.pack('>I', zlib.crc32(c) & 0xffffffff)

    ihdr = struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)  # 8-bit RGB
    png = (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', ihdr)
           + chunk(b'IDAT', zlib.compress(raw, 6)) + chunk(b'IEND', b''))
    open(dst_path, 'wb').write(png)
    print(f"PNG {dst_path} {w}x{h}")

if __name__ == '__main__':
    bmp_to_png(sys.argv[1], sys.argv[2])
