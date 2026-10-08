"""Prototype decoder for .mar joint-rotation tracks (flag 0x02).

Mirrors FUN_003b1930 / FUN_003b2be0 / FUN_003b2d40 / FUN_003b2aa8 in the w00a
resident image. Usage: py -I mar_proto.py file.mar
"""
import math
import struct
import sys


class Bits:
    """Little-endian u16 stream read 32 bits at a time, like the game."""

    def __init__(self, data, halfword):
        self.d, self.w, self.bit = data, halfword, 0

    def read(self, n):
        lo, hi = struct.unpack_from('<HH', self.d, self.w * 2)
        v = ((lo | hi << 16) >> self.bit) & ((1 << n) - 1)
        self.bit += n
        self.w += self.bit >> 4
        self.bit &= 15
        return v


def s12(v):
    return (v | ~0xFFF) if v & 0x800 else v


def quat(bits):
    return [s12(bits.read(12)) / 2047.0 for _ in range(4)]


def parse(b):
    assert b[:4] == b'MARa', b[:4]
    n, base = struct.unpack_from('<II', b, 8)
    motions = []
    for i in range(n):
        off, _ = struct.unpack_from('<II', b, 0x10 + i * 8)
        m = base + off
        hdr = struct.unpack_from('<11I', b, m)
        motions.append((m, hdr))
    return motions


def joint_keys(b, m, hdr, joint):
    flags, step, length, data_off = hdr[1], hdr[2], hdr[3], hdr[4]
    njoints = hdr[6]
    off = struct.unpack_from('<I', b, m + 0x2C + joint * 4)[0]
    data = m + data_off
    bits = Bits(b, 0)
    bits.w = (data // 2) + off  # data is 2-aligned
    assert data % 2 == 0
    keys, t, total = [], 0, step * length
    while t <= total and len(keys) < 400:
        d = bits.read(8)
        a = quat(bits)
        if d & 0x80:
            d &= 0x7F
            c = a
        else:
            c = quat(bits)
        keys.append((t, d, a, c))
        t += d * step
    return keys


b = open(sys.argv[1], 'rb').read()
ms = parse(b)
print(len(ms), 'motions')
for idx, (m, hdr) in enumerate(ms[:6]):
    flags = hdr[1]
    print(f'motion {idx}: id={hdr[0]:#x} flags={flags:#x} step={hdr[2]} len={hdr[3]} '
          f'joints={hdr[6]} offs={[hex(x) for x in hdr[7:11]]}')
    if flags & 2:
        for j in (0, 1, 2):
            ks = joint_keys(b, m, hdr, j)
            norms = [round(math.sqrt(sum(x * x for x in k[2])), 3) for k in ks[:6]]
            print(f'   joint {j}: {len(ks)} keys, durations {[k[1] for k in ks[:8]]}, |A| {norms}')
            print('      first A', [round(x, 3) for x in ks[0][2]], 'C', [round(x, 3) for x in ks[0][3]])
