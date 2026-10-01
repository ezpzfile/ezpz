#!/usr/bin/env python3
"""Writes input-x64big/big64 next to this script: a synthetic 9,000,000-byte x86-64 file (not a
real program) for transform 4 with B = 24 (SPEC.md §9). It is mostly zero bytes, with a short
code-like snippet every 64 KiB: CALL and RIP-relative operands whose displacements reach more
than 8 MiB, a few values outside the converted range, and JMP (E9) and Jcc (0F 8x), which
transform 4 leaves alone. The last 12 bytes hold two patterns that end exactly at the end of
the file. x86-64-split-big.ezpz was made from this file with `ezpz c ... -l 7`."""
import os
import random
import struct

rng = random.Random(2026)
n = 9_000_000
b = bytearray(n)
b[0:4] = b"\x7fELF"
b[4], b[5], b[6] = 2, 1, 1                      # 64-bit, little-endian, version 1
b[16:20] = struct.pack("<HH", 2, 0x3E)          # executable, x86-64
targets = [rng.randrange(n) for _ in range(40)]


def put(p, opcode, end, t):
    """Writes opcode bytes and a rel32 that points from `end` to `t` (or a far value)."""
    v = (t - end) & 0xFFFFFFFF if t is not None else rng.getrandbits(32) | 0x40000000
    b[p:p + len(opcode)] = opcode
    b[p + len(opcode):p + len(opcode) + 4] = struct.pack("<I", v)
    return p + len(opcode) + 4


for base in range(4096, n - 4096, 65536):
    p = base
    for _ in range(4):
        t = rng.choice(targets) if rng.random() < 0.9 else None
        k = rng.randrange(6)
        if k == 0:
            p = put(p, b"\xE8", p + 5, t)                   # CALL rel32
        elif k == 1:
            p = put(p, b"\x48\x8B\x05", p + 7, t)           # MOV RAX, [RIP+disp32]
        elif k == 2:
            p = put(p, b"\x0F\x10\x05", p + 7, t)           # MOVUPS XMM0, [RIP+disp32]
        elif k == 3:
            p = put(p, b"\xF2\x0F\x10\x05", p + 8, t)       # MOVSD XMM0, [RIP+disp32]
        elif k == 4:
            p = put(p, b"\xE9", p + 5, t)                   # JMP rel32: left alone
        else:
            p = put(p, b"\x0F\x85", p + 6, t)               # JNE rel32: left alone
        b[p] = 0x90                                         # NOP
        p += 1
put(n - 12, b"\x48\x8B\x05", n - 5, targets[0])
put(n - 5, b"\xE8", n, targets[1])

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "input-x64big")
os.makedirs(out, exist_ok=True)
with open(os.path.join(out, "big64"), "wb") as f:
    f.write(b)
