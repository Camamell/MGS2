"""Prototype .hzx walker: dumps structure and plots element vertices top-down.

Usage: python hzx_proto.py file.hzx out.png
"""
import struct
import sys

from PIL import Image, ImageDraw

b = open(sys.argv[1], 'rb').read()
u16 = lambda o: struct.unpack_from('<H', b, o)[0]
s16 = lambda o: struct.unpack_from('<h', b, o)[0]
u32 = lambda o: struct.unpack_from('<I', b, o)[0]

n_areas = u16(2)
print('hdr', [u16(i) for i in range(0, 8, 2)], [hex(u32(i)) for i in range(8, 0x20, 4)])
areas = u32(0x1C)
polys = {'floor': [], 'wall': [], 'floor2': [], 'wall2': []}
for a in range(n_areas):
    ao = areas + a * 0x5C
    words = struct.unpack_from('<23I', b, ao)
    ngroups = words[0x14] & 0xFFFF
    print(f'area {a}: words', [hex(x) for x in words])
    for g in range(ngroups):
        go = words[0x15] + g * 0x34
        counts = struct.unpack_from('<5H', b, go + 0x14)
        ptrs = struct.unpack_from('<5I', b, go + 0x20)
        if g < 3:
            print(f'  group {g}: head', [hex(x) for x in struct.unpack_from('<5I', b, go)], 'counts', counts,
                  'u16@1e', u16(go + 0x1E), 'ptrs', [hex(p) for p in ptrs])
        for kind, cnt, p in zip(polys, counts, ptrs):
            for e in range(cnt):
                eo = p + e * 0x20
                n = s16(eo + 6)
                data = u32(eo + 0x10)
                verts = [struct.unpack_from('<4h', b, data + i * 8) for i in range(n)]
                if g == 0 and e < 2:
                    print(f'    {kind} elem {e}: raw', b[eo:eo + 0x20].hex(' '))
                    print('      verts', verts[:6])
                polys[kind].append(verts)

# top-down plot (x, z)
allv = [v for k in polys for poly in polys[k] for v in poly]
xs = [v[0] for v in allv]; zs = [v[2] for v in allv]
mnx, mxx, mnz, mxz = min(xs), max(xs), min(zs), max(zs)
S = 1000
sc = S / max(mxx - mnx, mxz - mnz, 1)
img = Image.new('RGB', (S, S), (20, 20, 20))
d = ImageDraw.Draw(img)
col = {'floor': (60, 160, 60), 'floor2': (60, 100, 200), 'wall': (220, 80, 60), 'wall2': (220, 200, 60)}
for k in ('floor', 'floor2', 'wall', 'wall2'):
    for poly in polys[k]:
        pts = [((v[0] - mnx) * sc, (v[2] - mnz) * sc) for v in poly]
        if len(pts) >= 2:
            d.line(pts + [pts[0]], fill=col[k], width=1)
img.save(sys.argv[2])
print({k: len(v) for k, v in polys.items()}, 'extent x', mnx, mxx, 'z', mnz, mxz, 'y', min(v[1] for v in allv), max(v[1] for v in allv))
