"""Prototype: list (and optionally extract) the files in one STAGE.DAT stage.

Usage: py -I stage_proto.py STAGE.DAT <stage> [outdir]
"""
import os
import struct
import sys
import zlib

M = 0xFFFFFFFF


class Cipher:
    def __init__(self, key, step):
        self.key, self.step = key & M, step & M

    def apply(self, data):
        # The game decrypts whole words, including a trailing partial one.
        n = len(data)
        out = bytearray(data) + bytes(-n % 4)
        for i in range(0, len(out), 4):
            w, = struct.unpack_from('<I', out, i)
            struct.pack_into('<I', out, i, w ^ self.key)
            self.key = (self.key * 0x2E90EDD + self.step) & M
        return out[:n]


def str_hash(name: bytes):
    h = 0
    for c in name:
        h = (((h << 5) | (h >> 0x13)) + c) & 0xFFFFFF
    return h or 1


def read_toc(f):
    f.seek(0)
    head = f.read(0x800)
    seed, = struct.unpack_from('<I', head, 0)
    c = Cipher(seed, seed ^ 0xF0F0)
    w1, w2, _ = struct.unpack('<III', c.apply(head[4:16]))
    count = w2 & 0xFFFF
    f.seek(0)
    blob = f.read((w1 >> 16) << 11)
    toc = c.apply(blob[16:16 + count * 12])
    return seed, {toc[i*12:i*12+8].rstrip(b'\0').decode('latin1'): struct.unpack_from('<I', toc, i*12+8)[0]
                  for i in range(count)}


def main():
    f = open(sys.argv[1], 'rb')
    stage = sys.argv[2]
    out = sys.argv[3] if len(sys.argv) > 3 else None
    seed, toc = read_toc(f)
    base = toc[stage] * 0x800
    h = str_hash(stage.encode())
    c = Cipher(h * 0x81 + seed - 0x5876DA27, h * 0x81 + 0x7A88FB59)
    f.seek(base)
    first = c.apply(f.read(0x800))
    n, = struct.unpack_from('<I', first, 0)
    hdr_len = n * 8 + 4
    if hdr_len > 0x800:
        f.seek(base + 0x800)
        first += c.apply(f.read(((hdr_len + 0x7FF) // 0x800) * 0x800 - 0x800))
    ents = [struct.unpack_from('<II', first, 4 + i * 8) for i in range(n)]
    print(f'{stage}: base={base:#x} entries={n}')
    pos = (hdr_len + 0x7FF) & ~0x7FF
    i = 0
    page_no = 0
    while i < n:
        a, b = ents[i]
        if not ((0x7F000000 < a < 0x7F000004) or a == 0x7F010000):
            i += 1
            continue
        nxt = ents[i + 1] if i + 1 < n else (0, 0)
        compressed = (nxt[0] & 0xFF000000) == 0x7E000000
        stored = (nxt[0] & 0xFFFFFF) if compressed else b
        f.seek(base + pos)
        raw = f.read(stored)
        if compressed:
            v, = struct.unpack_from('<H', raw, 0)
            k = (v ^ 0x9385) & 0xFFFF
            c2 = Cipher(k | (((v ^ 0xF6F3) & 0xFFFF) << 16), k * 0x116)
            dec = c2.apply(raw)
            struct.pack_into('<H', dec, 0, (struct.unpack_from('<H', dec, 0)[0] + 0x8F3) & 0xFFFF)
            data = zlib.decompress(bytes(dec))
        else:
            data = raw
        print(f' page {page_no} kind={a:#x} at {pos:#x} stored={stored:#x} size={len(data):#x} '
              f'{"zlib" if compressed else "raw"} (declared {b:#x})')
        j = i + 1 + (1 if compressed else 0)
        while j < n and ents[j][0] != 0x7F000000:
            fid, off = ents[j]
            end = ents[j + 1][1]
            typ = (fid & 0x7FFFFFFF) >> 24
            ext = chr(ord('a') + typ) if typ < 26 else f't{typ}'
            body = data[off:end]
            print(f'   {fid:08x} type={typ:2d}({ext}) off={off:#8x} size={end-off:#8x} magic={body[:8].hex()}')
            if out:
                d = os.path.join(out, stage, f'p{page_no:02d}')
                os.makedirs(d, exist_ok=True)
                open(os.path.join(d, f'{fid & 0xFFFFFF:06x}.{ext}'), 'wb').write(body)
            j += 1
        pos += (stored + 0x7FF) & ~0x7FF
        page_no += 1
        i = j + 1


main()
