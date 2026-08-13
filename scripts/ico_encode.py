#!/usr/bin/env python3
"""Кодирует app-icon.ico: малые размеры (16-128) как BMP/DIB, 256 как PNG.

ПОЧЕМУ: make-icon.sh раньше заворачивал ВСЕ размеры как PNG-blob'ы в ICO.
Windows для малых/средних значков (16/32/48 — таскбар, список, проводник) ждёт
BMP/DIB; PNG-in-ICO надёжно работает только для 256. Все-PNG ICO → Windows
показывает дефолтную иконку (не бренд). .NET Icon-лоадер тоже падает на нём.

ЭТОТ СКРИПТ: PNG → декодируем (zlib) → BMP-DIB (BITMAPINFOHEADER + BGRA
bottom-up + AND-mask) для 16/24/32/48/64/128; 256 оставляем PNG. Без Pillow/
icotool/ImageMagick (их нет в MSYS2-окружении).

Использование: python3 ico_encode.py <ico_out> <png256> <png128> <png64> ...
Размеры определяются по фактическому размеру PNG (читаем из IHDR).
"""
import sys, zlib, struct


def load_png(path):
    """Минимальный PNG-декодер (8-bit RGB type 2 / RGBA type 6). Возвращает (w,h,rgba_bytes top-down)."""
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", f"not a PNG: {path}"
    pos = 8
    width = height = depth = ctype = None
    idat = bytearray()
    while pos < len(data):
        (ln,) = struct.unpack(">I", data[pos:pos + 4]); pos += 4
        typ = data[pos:pos + 4]; pos += 4
        chunk = data[pos:pos + ln]; pos += ln + 4  # +CRC
        if typ == b"IHDR":
            width, height, depth, ctype = struct.unpack(">IIBB", chunk[:10])
        elif typ == b"IDAT":
            idat += chunk
        elif typ == b"IEND":
            break
    assert depth == 8, f"unsupported bit depth {depth} in {path}"
    assert ctype in (2, 6), f"unsupported color type {ctype} in {path}"
    bpp = 3 if ctype == 2 else 4
    raw = zlib.decompress(bytes(idat))
    stride = width * bpp
    out = bytearray()
    prev = bytearray(stride)
    p = 0
    for _y in range(height):
        ft = raw[p]; p += 1
        line = bytearray(raw[p:p + stride]); p += stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
            if ft == 1:
                line[x] = (line[x] + a) & 255
            elif ft == 2:
                line[x] = (line[x] + b) & 255
            elif ft == 3:
                line[x] = (line[x] + ((a + b) >> 1)) & 255
            elif ft == 4:
                pr = a + b - c
                pa, pb, pc = abs(pr - a), abs(pr - b), abs(pr - c)
                line[x] = (line[x] + (a if (pa <= pb and pa <= pc) else (b if pb <= pc else c))) & 255
            # ft == 0: None
        out += line
        prev = line
    # привести к RGBA top-down
    rgba = bytearray(width * height * 4)
    for i in range(width * height):
        if ctype == 6:
            r, g, b, al = out[i * 4:i * 4 + 4]
        else:
            r, g, b = out[i * 3:i * 3 + 3]; al = 255
        rgba[i * 4:i * 4 + 4] = bytes((r, g, b, al))
    return width, height, bytes(rgba)


def dib_entry(width, height, rgba):
    """BMP-DIB для ICO-записи: BITMAPINFOHEADER(40) + BGRA bottom-up + AND-mask (1bpp)."""
    # AND mask: 1 бит/пиксель, строки добиваются до 4 байт, bottom-up. 1 = прозрачно.
    mask_stride = ((width + 31) // 32) * 4
    xor = bytearray(width * height * 4)  # BGRA, bottom-up
    mask = bytearray(mask_stride * height)
    for y in range(height):
        src_y = height - 1 - y  # bottom-up
        for x in range(width):
            r, g, b, al = rgba[(src_y * width + x) * 4:(src_y * width + x) * 4 + 4]
            xor[(y * width + x) * 4:(y * width + x) * 4 + 4] = bytes((b, g, r, al))
            if al < 128:
                mask[y * mask_stride + (x >> 3)] |= 0x80 >> (x & 7)
    bi_size_image = len(xor) + len(mask)
    hdr = struct.pack("<IiiHHIIiiII",
                      40, width, height * 2, 1, 32, 0, bi_size_image, 0, 0, 0, 0)
    return hdr + bytes(xor) + bytes(mask)


def main():
    out = sys.argv[1]
    pngs = sys.argv[2:]
    entries = []  # (width, dib_or_png_bytes, is_png)
    for path in pngs:
        w, h, rgba = load_png(path)
        assert w == h and w in (16, 24, 32, 48, 64, 128, 256), f"unexpected size {w}x{h}"
        if w == 256:
            # PNG-blob для 256 (надёжно поддерживается Windows Vista+)
            entries.append((w, open(path, "rb").read(), True))
        else:
            entries.append((w, dib_entry(w, h, rgba), False))
    # сортировка по возрастанию размера (стандартное соглашение ICO)
    entries.sort(key=lambda e: e[0])
    n = len(entries)
    dir_size = 6 + 16 * n
    # смещения
    blobs = []
    offsets = []
    cur = dir_size
    for w, blob, is_png in entries:
        offsets.append(cur)
        blobs.append(blob)
        cur += len(blob)
    # заголовок + директория
    head = struct.pack("<HHH", 0, 1, n)
    directory = b""
    for (w, blob, is_png), off in zip(entries, offsets):
        wb = 0 if w == 256 else w
        directory += struct.pack("<BBBBHHII", wb, wb, 0, 0, 1, 32, len(blob), off)
    with open(out, "wb") as f:
        f.write(head)
        f.write(directory)
        for blob in blobs:
            f.write(blob)
    print(f"Создан {out} ({cur} байт, {n} размеров: малые BMP/DIB + 256 PNG)")


if __name__ == "__main__":
    main()
