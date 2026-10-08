"""Print decompiled functions from all.c by name or address.

Usage: py -I fn.py <all.c> NAME_OR_ADDR [...]
       py -I fn.py <all.c> --callers NAME   (functions that call NAME)
"""
import re
import sys

src = open(sys.argv[1], encoding='utf-8', errors='replace').read()
parts = re.split(r'^// ==== ', src, flags=re.M)[1:]
funcs = {}
for p in parts:
    head, _, body = p.partition('\n')
    addr, name = head.split(' ', 1)
    funcs[name.strip()] = (addr, body)
    funcs[addr] = (addr, body)

args = sys.argv[2:]
if args and args[0] == '--callers':
    target = args[1]
    seen = set()
    for name, (addr, body) in funcs.items():
        if addr in seen or name == addr:
            continue
        if re.search(r'\b' + re.escape(target) + r'\s*\(', body) and name != target:
            seen.add(addr)
            print(addr, name)
    sys.exit()
for a in args:
    key = a if a in funcs else 'FUN_' + a.lower().removeprefix('0x').zfill(8)
    if key not in funcs:
        print('// not found', a)
        continue
    addr, body = funcs[key]
    print('// ====', addr, key)
    print(body)
