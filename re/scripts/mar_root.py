"""Decode the root-position track (flag 0x20) of every motion in a .mar and
print displacement and speed, to tell idle/walk/run apart.

Track layout (FUN_003B1650 / FUN_003B1E50): at motion data + 2*hdr[0x20]:
  3 x fp16 start position, then keys from bit 48:
  u8 duration (frames), 3 x fp16 position, 3 x fp16 tangent.
"""
import struct
import sys


def f16(v):
    sign = -1.0 if v & 0x8000 else 1.0
    e, m = (v >> 10) & 0x1F, v & 0x3FF
    if e == 0:
        return 0.0 if m == 0 else sign * m / 1024 * 2 ** -14
    return sign * (1 + m / 1024) * 2 ** (e - 15)


class Bits:
    def __init__(self, d, word):
        self.d, self.w, self.bit = d, word, 0

    def read(self, n):
        lo, hi = struct.unpack_from('<HH', self.d, self.w * 2)
        v = ((lo | hi << 16) >> self.bit) & ((1 << n) - 1)
        self.bit += n
        self.w += self.bit >> 4
        self.bit &= 15
        return v


b = open(sys.argv[1], 'rb').read()
n, base = struct.unpack_from('<II', b, 8)
for i in range(n):
    m = base + struct.unpack_from('<I', b, 0x10 + i * 8)[0]
    hdr = struct.unpack_from('<11I', b, m)
    mid, flags, step, length, data = hdr[0], hdr[1], hdr[2], hdr[3], m + hdr[4]
    if not flags & 0x20:
        print(f'{i:3} id {mid:#06x} flags {flags:#x} len {length:4}  (no root curve)')
        continue
    bits = Bits(b, data // 2 + hdr[8])
    start = [f16(bits.read(16)) for _ in range(3)]
    keys, frame = [], 0
    while frame < length and len(keys) < 500:
        d = bits.read(8)
        p = [f16(bits.read(16)) for _ in range(3)]
        t = [f16(bits.read(16)) for _ in range(3)]
        frame += d
        keys.append((frame, p))
        if d == 0:
            break
    end = keys[-1][1] if keys else start
    dx, dz = end[0] - start[0], end[2] - start[2]
    dist = (dx * dx + dz * dz) ** 0.5
    speed = dist / max(length, 1) * 60 / 1000
    print(f'{i:3} id {mid:#06x} flags {flags:#x} len {length:4} keys {len(keys):3} '
          f'start y {start[1]:7.1f}  disp ({dx:7.1f},{end[1]-start[1]:7.1f},{dz:7.1f})  {speed:5.2f} m/s')
