"""Decrypt and print the STAGE.DAT table of contents (prototype for the Rust extractor)."""
import struct
import sys

M = 0xFFFFFFFF


class Cipher:
    def __init__(self, key, step):
        self.key, self.step = key & M, step & M

    def apply(self, data):
        out = bytearray(data)
        for i in range(0, len(out) - 3, 4):
            w, = struct.unpack_from('<I', out, i)
            struct.pack_into('<I', out, i, w ^ self.key)
            self.key = (self.key * 0x2E90EDD + self.step) & M
        return bytes(out)


def str_hash(name: bytes):
    h = 0
    for c in name:
        h = (((h << 5) | (h >> 0x13)) + c) & 0xFFFFFF
    return h or 1


f = open(sys.argv[1], 'rb')
head = f.read(0x800)
seed, = struct.unpack_from('<I', head, 0)
c = Cipher(seed, seed ^ 0xF0F0)
w1, count, w3 = struct.unpack('<III', c.apply(head[4:16]))
hdr_bytes = (w1 >> 16) << 11
print(f'seed={seed:08x} w1={w1:08x} count={count} w3={w3:08x} header_bytes={hdr_bytes:#x}')
f.seek(0)
blob = f.read(hdr_bytes)
toc = c.apply(blob[16:16 + count * 12])
for i in range(count):
    name, off = struct.unpack_from('<8sI', toc, i * 12)
    print(i, name.rstrip(b'\0').decode('latin1'), hex(off))
