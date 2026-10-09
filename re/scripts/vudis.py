"""Minimal PS2 VU micro-mode disassembler. Usage: vudis.py file offset count"""
import struct, sys

BC = 'xyzw'
UP = {}
for i, n in enumerate(['ADD', 'SUB', 'MADD', 'MSUB', 'MAX', 'MINI', 'MUL']):
    for b in range(4):
        UP[i * 4 + b] = (n + BC[b], 'bc')
UP.update({0x1c: ('MULq', 'q'), 0x1d: ('MAXi', 'i'), 0x1e: ('MULi', 'i'), 0x1f: ('MINIi', 'i'),
           0x20: ('ADDq', 'q'), 0x21: ('MADDq', 'q'), 0x22: ('ADDi', 'i'), 0x23: ('MADDi', 'i'),
           0x24: ('SUBq', 'q'), 0x25: ('MSUBq', 'q'), 0x26: ('SUBi', 'i'), 0x27: ('MSUBi', 'i'),
           0x28: ('ADD', '3'), 0x29: ('MADD', '3'), 0x2a: ('MUL', '3'), 0x2b: ('MAX', '3'),
           0x2c: ('SUB', '3'), 0x2d: ('MSUB', '3'), 0x2e: ('OPMSUB', '3'), 0x2f: ('MINI', '3')})
US = {}
for i, n in enumerate(['ADDA', 'SUBA', 'MADDA', 'MSUBA']):
    for b in range(4):
        US[i * 4 + b] = (n + BC[b], 'abc')
for b in range(4):
    US[0x18 + b] = ('MULA' + BC[b], 'abc')
US.update({0x10: ('ITOF0', '2'), 0x11: ('ITOF4', '2'), 0x12: ('ITOF12', '2'), 0x13: ('ITOF15', '2'),
           0x14: ('FTOI0', '2'), 0x15: ('FTOI4', '2'), 0x16: ('FTOI12', '2'), 0x17: ('FTOI15', '2'),
           0x1c: ('MULAq', 'aq'), 0x1d: ('ABS', '2'), 0x1e: ('MULAi', 'ai'), 0x1f: ('CLIP', 'clip'),
           0x20: ('ADDAq', 'aq'), 0x21: ('MADDAq', 'aq'), 0x22: ('ADDAi', 'ai'), 0x23: ('MADDAi', 'ai'),
           0x24: ('SUBAq', 'aq'), 0x25: ('MSUBAq', 'aq'), 0x26: ('SUBAi', 'ai'), 0x27: ('MSUBAi', 'ai'),
           0x28: ('ADDA', 'a3'), 0x29: ('MADDA', 'a3'), 0x2a: ('MULA', 'a3'), 0x2c: ('SUBA', 'a3'),
           0x2d: ('MSUBA', 'a3'), 0x2e: ('OPMULA', 'a3'), 0x2f: ('NOP', 'nop')})

def dest(w):
    d = (w >> 21) & 15
    return ''.join(c for c, bit in zip('xyzw', (8, 4, 2, 1)) if d & bit)

def upper(w):
    fd, fs, ft = (w >> 6) & 31, (w >> 11) & 31, (w >> 16) & 31
    op = w & 0x3f
    d = dest(w)
    flags = ''.join(f for f, b in zip('IEMDT', (31, 30, 29, 28, 27)) if w >> b & 1)
    if op >= 0x3c:
        sop = ((w >> 6) & 31) << 2 | (w & 3)
        n, k = US.get(sop, ('?%x' % sop, '?'))
        if k == 'abc': s = f'{n}.{d} ACC, vf{fs}, vf{ft}{BC[w & 3]}'
        elif k == '2': s = f'{n}.{d} vf{ft}, vf{fs}'
        elif k == 'aq': s = f'{n}.{d} ACC, vf{fs}, Q'
        elif k == 'ai': s = f'{n}.{d} ACC, vf{fs}, I'
        elif k == 'a3': s = f'{n}.{d} ACC, vf{fs}, vf{ft}'
        elif k == 'clip': s = f'CLIPw.xyz vf{fs}, vf{ft}'
        else: s = n
    else:
        n, k = UP.get(op, ('?%x' % op, '?'))
        if k == 'bc': s = f'{n}.{d} vf{fd}, vf{fs}, vf{ft}{BC[w & 3]}'
        elif k == 'q': s = f'{n}.{d} vf{fd}, vf{fs}, Q'
        elif k == 'i': s = f'{n}.{d} vf{fd}, vf{fs}, I'
        else: s = f'{n}.{d} vf{fd}, vf{fs}, vf{ft}'
    return s, flags

def simm11(w):
    v = w & 0x7ff
    return v - 0x800 if v & 0x400 else v

def lower(w, pc):
    op = w >> 25
    ft, fs, fd = (w >> 16) & 31, (w >> 11) & 31, (w >> 6) & 31
    d = dest(w)
    fsf, ftf = 'xyzw'[(w >> 21) & 3], 'xyzw'[(w >> 23) & 3]
    br = lambda: pc + 1 + simm11(w)
    if op == 0x40:
        f = w & 0x3f
        if f >= 0x3c:
            s = ((w >> 6) & 31) << 2 | (w & 3)
            m = {0x30: f'MOVE.{d} vf{ft}, vf{fs}', 0x31: f'MR32.{d} vf{ft}, vf{fs}',
                 0x34: f'LQI.{d} vf{ft}, (vi{fs}++)', 0x35: f'SQI.{d} vf{fs}, (vi{ft}++)',
                 0x36: f'LQD.{d} vf{ft}, (--vi{fs})', 0x37: f'SQD.{d} vf{fs}, (--vi{ft})',
                 0x38: f'DIV Q, vf{fs}{fsf}, vf{ft}{ftf}', 0x39: f'SQRT Q, vf{ft}{ftf}',
                 0x3a: f'RSQRT Q, vf{fs}{fsf}, vf{ft}{ftf}', 0x3b: 'WAITQ',
                 0x3c: f'MTIR vi{ft}, vf{fs}{fsf}', 0x3d: f'MFIR.{d} vf{ft}, vi{fs}',
                 0x3e: f'ILWR.{d} vi{ft}, (vi{fs})', 0x3f: f'ISWR.{d} vi{ft}, (vi{fs})',
                 0x40: f'RNEXT', 0x41: 'RGET', 0x42: 'RINIT', 0x43: 'RXOR', 0x64: 'MFP',
                 0x68: 'XTOP', 0x69: 'XITOP', 0x6c: f'XGKICK vi{fs}', 0x72: f'ELENG P, vf{fs}',
                 0x73: f'ERLENG P, vf{fs}', 0x76: f'ESUM P, vf{fs}', 0x78: f'ESQRT P, vf{fs}{fsf}',
                 0x79: f'ERSQRT P, vf{fs}{fsf}', 0x7a: f'ERCPR P, vf{fs}{fsf}', 0x7b: 'WAITP',
                 0x70: f'ESADD P, vf{fs}', 0x71: f'ERSADD P, vf{fs}'}
            return m.get(s, '?lsp%x' % s) if not (s == 0x30 and w == 0x8000033c) else 'NOP'
        m = {0x30: f'IADD vi{fd}, vi{fs}, vi{ft}', 0x31: f'ISUB vi{fd}, vi{fs}, vi{ft}',
             0x32: f'IADDI vi{ft}, vi{fs}, {((w >> 6) & 31) - 32 if (w >> 10) & 1 else (w >> 6) & 31}',
             0x34: f'IAND vi{fd}, vi{fs}, vi{ft}', 0x35: f'IOR vi{fd}, vi{fs}, vi{ft}'}
        return m.get(f, '?l40_%x' % f)
    imm15 = (w & 0x7ff) | ((w >> 21) & 15) << 11
    m = {0x00: f'LQ.{d} vf{ft}, {simm11(w)}(vi{fs})', 0x01: f'SQ.{d} vf{fs}, {simm11(w)}(vi{ft})',
         0x04: f'ILW.{d} vi{ft}, {simm11(w)}(vi{fs})', 0x05: f'ISW.{d} vi{ft}, {simm11(w)}(vi{fs})',
         0x08: f'IADDIU vi{ft}, vi{fs}, {imm15}', 0x09: f'ISUBIU vi{ft}, vi{fs}, {imm15}',
         0x10: f'FCEQ vi1, {w & 0xffffff:#x}', 0x11: f'FCSET {w & 0xffffff:#x}',
         0x12: f'FCAND vi1, {w & 0xffffff:#x}', 0x13: f'FCOR vi1, {w & 0xffffff:#x}',
         0x16: f'FSAND vi{ft}, {imm15:#x}', 0x1a: f'FMAND vi{ft}, vi{fs}', 0x1c: f'FCGET vi{ft}',
         0x20: f'B {br()}', 0x21: f'BAL vi{ft}, {br()}', 0x24: f'JR vi{fs}', 0x25: f'JALR vi{ft}, vi{fs}',
         0x28: f'IBEQ vi{ft}, vi{fs}, {br()}', 0x29: f'IBNE vi{ft}, vi{fs}, {br()}',
         0x2c: f'IBLTZ vi{fs}, {br()}', 0x2d: f'IBGTZ vi{fs}, {br()}', 0x2e: f'IBLEZ vi{fs}, {br()}',
         0x2f: f'IBGEZ vi{fs}, {br()}'}
    return m.get(op, '?l%x' % op)

def main():
    data = open(sys.argv[1], 'rb').read()
    off, count = int(sys.argv[2], 0), int(sys.argv[3], 0)
    for pc in range(count):
        lo, hi = struct.unpack_from('<II', data, off + pc * 8)
        u, flags = upper(hi)
        l = f'LOI {struct.unpack("<f", struct.pack("<I", lo))[0]}' if hi >> 31 & 1 else lower(lo, pc)
        print(f'{pc:4d} {pc*8:04x}  {u:34s} {l:34s} {flags}')

main()
