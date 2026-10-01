#!/usr/bin/env python3
"""Independent clean-room .ezpz reader, written only from SPEC.md (v1.0 draft, round-2 text).
Codec 4 (brain-fast, §6.5) was added later, also from SPEC.md alone; it was then updated to the
ezpz 0.3.0 layout (2-byte model mask, any subset of models 0..8), together with transform 3
(x86-64, §9), again from SPEC.md alone. The priming flag P (§6.4.1 byte 0 bit 7) and priming
(§6.4.11, Appendix B; codec 4 per §6.5) were added later, again from SPEC.md alone.

usage: reader.py ARCHIVE [--out DIR] [--compare DIR --sub SUB] [--pub HEXFILE]
                 [--password PW] [--brain-impl fast|ref] [--prime FILE]

Without --password, an encrypted archive is verified as far as the spec allows
without the key (§14: steps 1-7 + every block hash) and then stops.
"""
import os
import sys
import struct
import argparse
import lzma
from operator import mul

import blake3
import zstandard
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
from cryptography.exceptions import InvalidSignature

M32 = 0xFFFFFFFF
I32MAX = 0x7FFFFFFF
I32MIN = -0x80000000
LOG = []


class FormatError(Exception):
    pass


def need(cond, msg):
    if not cond:
        raise FormatError(msg)


def log(*a):
    s = " ".join(str(x) for x in a)
    LOG.append(s)
    print(s, flush=True)


def b3(data):
    return blake3.blake3(data).digest()


# ---------------------------------------------------------------- byte reader (§1)
class Rd:
    def __init__(self, b, what="?"):
        self.b = b
        self.p = 0
        self.what = what
        self.nonminimal = 0

    def take(self, n):
        need(n >= 0 and self.p + n <= len(self.b), f"{self.what}: truncated (need {n} at {self.p})")
        r = self.b[self.p:self.p + n]
        self.p += n
        return r

    def u8(self):
        return self.take(1)[0]

    def u16(self):
        return struct.unpack("<H", self.take(2))[0]

    def u32(self):
        return struct.unpack("<I", self.take(4))[0]

    def u64(self):
        return struct.unpack("<Q", self.take(8))[0]

    def varint(self):
        # §1: LEB128, at most 10 bytes / 64 bits (MUST); non-minimal accepted (MAY), but counted
        v = 0
        shift = 0
        nb = 0
        while True:
            c = self.u8()
            nb += 1
            need(nb <= 10, f"{self.what}: varint longer than 10 bytes")
            if nb == 10:
                need(c <= 1, f"{self.what}: varint > 64 bits")
            v |= (c & 0x7F) << shift
            shift += 7
            if not (c & 0x80):
                break
        if nb > 1 and c == 0:
            self.nonminimal += 1
        return v

    def svarint(self):
        u = self.varint()
        return (u >> 1) ^ -(u & 1)

    def eof(self):
        return self.p == len(self.b)


# ---------------------------------------------------------------- Ed25519 strict (§10)
P25519 = 2 ** 255 - 19
D25519 = (-121665 * pow(121666, P25519 - 2, P25519)) % P25519
L25519 = 2 ** 252 + 27742317777372353535851937790883648493


def _ed_decode(enc):
    y = int.from_bytes(enc, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    if y >= P25519:
        return None, "non-canonical y"
    u = (y * y - 1) % P25519
    v = (D25519 * y * y + 1) % P25519
    x2 = u * pow(v, P25519 - 2, P25519) % P25519
    x = pow(x2, (P25519 + 3) // 8, P25519)
    if (x * x - x2) % P25519 != 0:
        x = x * pow(2, (P25519 - 1) // 4, P25519) % P25519
    if (x * x - x2) % P25519 != 0:
        return None, "not on curve"
    if x == 0 and sign:
        return None, "x=0 with sign bit"
    if (x & 1) != sign:
        x = P25519 - x
    return (x, y, 1, x * y % P25519), None


def _ed_add(Pt, Qt):
    X1, Y1, Z1, T1 = Pt
    X2, Y2, Z2, T2 = Qt
    p = P25519
    A = (Y1 - X1) * (Y2 - X2) % p
    B = (Y1 + X1) * (Y2 + X2) % p
    C = 2 * D25519 * T1 * T2 % p
    Dd = 2 * Z1 * Z2 % p
    E, F, G, H = B - A, Dd - C, Dd + C, B + A
    return (E * F % p, G * H % p, F * G % p, E * H % p)


def _ed_small_order(Pt):
    Q = Pt
    for _ in range(3):
        Q = _ed_add(Q, Q)
    X, Y, Z, _ = Q
    return X % P25519 == 0 and (Y - Z) % P25519 == 0


def ed25519_verify_strict(pk, sig, msg):
    for name, enc in (("public key A", pk), ("R", sig[:32])):
        pt, err = _ed_decode(enc)
        need(pt is not None, f"signature: {name} invalid ({err})")
        need(not _ed_small_order(pt), f"signature: {name} has small order")
    S = int.from_bytes(sig[32:], "little")
    need(S < L25519, "signature: S not canonical (S >= L)")
    try:  # OpenSSL: cofactorless [S]B = R + [k]A
        Ed25519PublicKey.from_public_bytes(pk).verify(sig, msg)
    except InvalidSignature:
        raise FormatError("signature: Ed25519 verification FAILED")


# ---------------------------------------------------------------- brain codec (§6.4)
SQ_T = [1, 2, 3, 6, 10, 16, 27, 45, 73, 120, 194, 310, 488, 747, 1101, 1546,
        2047, 2549, 2994, 3348, 3607, 3785, 3901, 3975, 4022, 4050, 4068, 4079,
        4085, 4089, 4092, 4093, 4094]


def squash(d):
    if d > 2047:
        return 4095
    if d < -2047:
        return 1
    w = d & 127
    i = (d >> 7) + 16
    return (SQ_T[i] * (128 - w) + SQ_T[i + 1] * w + 64) >> 7


def _build_stretch():
    S = [0] * 4096
    pi = 0
    for x in range(-2047, 2048):
        v = squash(x)
        for i in range(pi, v + 1):
            S[i] = x
        pi = max(pi, v + 1)
    for i in range(pi, 4096):
        S[i] = 2047
    return S


STRETCH = _build_stretch()
# squash lookup for d in [-2048, 2047]; NB: the averaged st can reach -2048 (3 * -2047 * 21846 >> 16)
SQTAB = [squash(d) for d in range(-2048, 2048)]


def hash2(a, b):
    h = (a * 0x9E3779B1 + b * 0x85EBCA6B) & M32
    h ^= h >> 16
    h = (h * 0x2C1B3C6D) & M32
    h ^= h >> 13
    return h


RECIP = [131072 // (2 * n + 3) for n in range(1024)]


def p12(cnt):
    return ((cnt >> 10) ^ 0x200000) >> 10


def cnt_update(cnt, y, limit):
    n = cnt & 1023
    p = (cnt >> 10) ^ 0x200000
    target = (1 << 22) - 1 if y else 0
    p = p + (((target - p) * RECIP[n]) >> 16)
    if n < limit:
        n += 1
    return ((p ^ 0x200000) << 10) | n


def _next_state(s, y):
    n0 = s & 63
    n1 = (s >> 6) & 63
    if y == 1:
        n1 = min(n1 + 1, 63)
        if n0 > 2:
            n0 = (n0 >> 1) + 1
    else:
        n0 = min(n0 + 1, 63)
        if n1 > 2:
            n1 = (n1 >> 1) + 1
    return n0 | (n1 << 6) | (y << 12)


NEXT0 = [_next_state(s, 0) for s in range(8192)]
NEXT1 = [_next_state(s, 1) for s in range(8192)]
TOTAL = [(s & 63) + ((s >> 6) & 63) for s in range(8192)]


def wrap32(v):
    return ((v + 0x80000000) & M32) - 0x80000000


# ---------------------------------------------------------------- priming data (§6.4.11, Appendix B)
PRIME_V1_LEN = 75598
PRIME_V1_BLAKE3 = "05f3a9a79ebd0a489559afc99ece7634838fe5b11aa13f84d5030cb693c1b1c2"
PRIME_DEFAULT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                              os.pardir, "prime", "v1.txt"))
_PRIME_CACHE = {}


class PrimingDataError(FormatError):
    """The local priming data file is missing or is not the Appendix B file (not an archive defect)."""


def priming_data():
    """Appendix B: version 1 of the priming data D (prime/v1.txt). Loaded once, and checked against
    the stated size and BLAKE3-256 before first use ("Decoders MUST use exactly these bytes")."""
    path = OPTS["prime"]
    if path not in _PRIME_CACHE:
        try:
            with open(path, "rb") as fh:
                d = fh.read()
        except OSError as ex:
            raise PrimingDataError(f"priming data (Appendix B): cannot read {path}: {ex}")
        if len(d) != PRIME_V1_LEN:
            raise PrimingDataError(f"priming data (Appendix B): {path} is {len(d)} bytes, expected "
                                   f"{PRIME_V1_LEN}; refusing to decode a primed (P=1) block")
        h = b3(d).hex()
        if h != PRIME_V1_BLAKE3:
            raise PrimingDataError(f"priming data (Appendix B): {path} has BLAKE3 {h}, expected "
                                   f"{PRIME_V1_BLAKE3}; refusing to decode a primed (P=1) block")
        log(f"    priming data: {path} ({len(d)} bytes, size and BLAKE3 match Appendix B)")
        _PRIME_CACHE[path] = d
    return _PRIME_CACHE[path]


def brain_header(payload, codec):
    """Payload header of codec 3 (§6.4.1) or codec 4 (§6.5).
    Byte 0: bits 0..6 = tb, bit 7 = the priming flag P (§6.4.1; same byte layout for codec 4, §6.5).
    Returns (tb, primed, models, mask, src): primed = P (0 or 1), models = the context model
    indices in use, in increasing order (always 0..8 for codec 3), mask = the u16 model mask
    (None for codec 3), src = the arithmetic coder input (from byte 1 for codec 3, from byte 3
    for codec 4; priming does not move it, §6.4.11)."""
    need(codec in (3, 4), f"brain: codec {codec}")
    need(len(payload) >= 1, "brain: empty payload")
    tb = payload[0] & 0x7F
    primed = payload[0] >> 7
    need(16 <= tb <= 25, f"brain: tb={tb} out of range")
    if codec == 3:
        return tb, primed, list(range(9)), None, payload[1:]
    # §6.5: bytes 1-2 = model mask (u16 LE); bits 9..15 MUST be 0; any subset of 0..8 (even none)
    need(len(payload) >= 3, "brain-fast: payload shorter than tb + 2-byte model mask")
    mask = payload[1] | (payload[2] << 8)
    need(mask & 0xFE00 == 0, f"brain-fast: model mask {mask:#06x} has bits 9..15 set (MUST reject)")
    models = [i for i in range(9) if (mask >> i) & 1]
    return tb, primed, models, mask, payload[3:]


def all_ctx(c4, c8, word, pword):
    """The nine ctx_i of §6.4.5 (computed at the byte boundary)."""
    c1 = c4 & 0xFF
    return [c1, c4 & 0xFFFF, c4 & 0xFFFFFF, c4,
            hash2(c4, c8 & 0xFFFF),
            hash2(c1, 0x5757) if word == 0 else word,
            hash2(word, (pword + 0x3131) & M32),
            c4 & 0x00FFFF00, c4 & 0xFF0000FF]


def brain_decode_ref(payload, raw_len, codec=3):
    """Straight transcription of §6.4 (round-1 code). Slow but easy to audit.
    codec=4 applies the §6.5 changes (brain-fast): only the n context models named by the mask
    (each keeping its own index i in the bucket hash), an (n+3)-entry input vector,
    mixers A and C only (st = (stA + stC) >> 1), APM1 only.
    P = 1 (§6.4.11): starting from the initial state, every bit of the priming data D (Appendix B),
    most significant first, goes through prediction (§6.4.7) and learning (§6.4.8, with byte boundary
    processing) exactly like a decoded bit, but y is taken from D and the arithmetic decoder is not
    touched. D stays at the front of hist (the match model can point into it); only the block's
    own bytes go to out. Codec 4 primes with its own models (§6.5)."""
    tb, primed, models, mask, src = brain_header(payload, codec)
    light = codec == 4
    srclen = len(src)
    D = priming_data() if primed else b""
    nprime = len(D)                 # hist[0:nprime] = D, then the decoded bytes

    nm = len(models)                # context models: 0..8 (§6.4.5) or the mask's set bits (§6.5)
    ni = nm + 3                     # mixer inputs: models, order 0, match, bias (12 for codec 3)
    tables, shifts = [], []         # indexed by position k; models[k] is the model index i
    for i in models:
        bits = min(tb, 18) if i == 0 else tb
        tables.append([0] * (1 << bits))
        shifts.append(32 - (bits - 5))
    SM = [[0] * 8192 for _ in range(nm)]
    t0 = [0] * 256
    mt = [0] * (1 << (tb - 3))
    mshift = 32 - (tb - 3)
    m_ptr = 0
    m_len = 0
    m_sm = [0] * 64
    lq = 0
    WA = [16384] * (256 * ni)
    WB = None if light else [16384] * (256 * ni)     # §6.5: mixer B does not exist
    WC = [16384] * (256 * ni)
    row = [squash((j - 16) * 128) * 16 for j in range(33)]
    apm1 = row * 256
    apm2 = None if light else row * 65536            # §6.5: APM2 does not exist

    c0, nib, bitpos, c4, c8, word, pword = 1, 1, 0, 0, 0, 0, 0
    hist = bytearray()
    base = [0] * nm

    def pick_buckets(ctx, c0):
        for k, i in enumerate(models):               # §6.5: model i keeps its own index i in the hash
            tbl = tables[k]
            h = hash2((ctx[i] + (i << 28)) & M32, c0)
            L = h >> shifts[k]
            chk = (((h * 0x2545F491) & M32) >> 16) | 1
            ln = L * 32
            if tbl[ln] == chk:
                base[k] = ln
            elif tbl[ln + 16] == chk:
                base[k] = ln + 16
            else:
                w = 0 if TOTAL[tbl[ln + 1] & 0x1FFF] < TOTAL[tbl[ln + 17] & 0x1FFF] else 16
                b = ln + w
                for z in range(16):
                    tbl[b + z] = 0
                tbl[b] = chk
                base[k] = b

    def make_ctx():
        return all_ctx(c4, c8, word, pword)          # all nine; unused ones are simply not read

    x1, x2 = 0, M32
    x = 0
    sp = 0
    for _ in range(4):
        x = (x << 8) | (src[sp] if sp < srclen else 0)
        sp += 1

    pick_buckets(make_ctx(), c0)
    out = bytearray()
    xs = [0] * ni
    xs[ni - 1] = 256                # bias: x[11] (codec 3) / x[n+2] (codec 4)
    S = STRETCH
    idxs = [0] * nm
    sts = [0] * nm

    while len(out) < raw_len:
        for i in range(nm):         # i = position k: x[k] comes from model models[k] (§6.5)
            ix = base[i] + nib
            idxs[i] = ix
            s = tables[i][ix] & 0x1FFF
            sts[i] = s
            xs[i] = S[p12(SM[i][s])]
        xs[nm] = S[p12(t0[c0])]     # order 0: x[9] (codec 3) / x[n] (codec 4)
        xs[nm + 1] = 0              # match: x[10] (codec 3) / x[n+1] (codec 4)
        mctx = -1
        if m_len > 0:
            pb = hist[m_ptr] | 0x100
            sh = 8 - bitpos
            if (pb >> sh) == c0:
                e = (pb >> (sh - 1)) & 1
                mctx = lq * 2 + e
                xs[nm + 1] = S[p12(m_sm[mctx])]
        c1 = c4 & 0xFF
        selA = c0
        selB = (0 if mctx < 0 else lq) * 8 + bitpos
        selC = c1
        if light:
            mixers = ((WA, selA), (WC, selC))
        else:
            mixers = ((WA, selA), (WB, selB), (WC, selC))
        mst = []
        prs = []
        for W, sel in mixers:
            o = sel * ni
            dot = 0
            for i in range(ni):
                dot += xs[i] * W[o + i]
            st = dot >> 16
            if st > 2047:
                st = 2047
            elif st < -2047:
                st = -2047
            prs.append(squash(st))
            mst.append(st)
        if light:
            st = (mst[0] + mst[1]) >> 1                      # §6.5
        else:
            st = ((mst[0] + mst[1] + mst[2]) * 21846) >> 16  # §6.4.7
        pm = squash(st)
        s2 = st + 2048
        lo = s2 >> 7
        w = s2 & 127
        cx1 = c0 * 33
        idx1 = cx1 + lo + (w >> 6)
        a1 = (apm1[cx1 + lo] * (128 - w) + apm1[cx1 + lo + 1] * w) >> 11
        if light:
            p = (pm + 3 * a1 + 2) >> 2                       # §6.5: APM1 only
        else:
            cx2 = (c0 | (c1 << 8)) * 33
            idx2 = cx2 + lo + (w >> 6)
            a2 = (apm2[cx2 + lo] * (128 - w) + apm2[cx2 + lo + 1] * w) >> 11
            p = (pm + a1 + 2 * a2 + 2) >> 2
        if p < 1:
            p = 1
        elif p > 4095:
            p = 4095

        if len(hist) < nprime:
            # §6.4.11 priming: y is the next bit of D (most significant first); nothing is coded
            y = (D[len(hist)] >> (7 - bitpos)) & 1
        else:
            rng = x2 - x1
            xmid = x1 + (rng >> 12) * p + (((rng & 0xFFF) * p) >> 12)
            if x <= xmid:
                y = 1
                x2 = xmid
            else:
                y = 0
                x1 = xmid + 1
            while ((x1 ^ x2) & 0xFF000000) == 0:
                x1 = (x1 << 8) & M32
                x2 = ((x2 << 8) & M32) | 255
                x = ((x << 8) & M32) | (src[sp] if sp < srclen else 0)
                sp += 1

        NX = NEXT1 if y else NEXT0
        for i in range(nm):
            smi = SM[i]
            s = sts[i]
            smi[s] = cnt_update(smi[s], y, 1023)
            tbl = tables[i]
            tbl[idxs[i]] = NX[tbl[idxs[i]] & 0x1FFF]
        t0[c0] = cnt_update(t0[c0], y, 60)
        if mctx >= 0:
            m_sm[mctx] = cnt_update(m_sm[mctx], y, 1023)
        for (W, sel), pr in zip(mixers, prs):
            err = ((y << 12) - pr) * 16
            o = sel * ni
            for i in range(ni):
                W[o + i] = wrap32(W[o + i] + ((xs[i] * err) >> 16))
        g = (y << 16) + (y << 7) - y - y
        apm1[idx1] += (g - apm1[idx1]) >> 7
        if not light:
            apm2[idx2] += (g - apm2[idx2]) >> 7
        c0 = c0 * 2 + y
        nib = nib * 2 + y
        bitpos += 1

        if bitpos == 8:
            c = c0 & 0xFF
            hist.append(c)
            if len(hist) > nprime:  # a decoded byte; priming bytes (§6.4.11) go to hist only
                out.append(c)
            c8 = ((c8 << 8) | (c4 >> 24)) & M32
            c4 = ((c4 << 8) | c) & M32
            lc = c + 32 if 0x41 <= c <= 0x5A else c
            if 0x61 <= lc <= 0x7A or c >= 0x80:
                word = hash2((word + lc) & M32, 0x77770001)
            elif word != 0:
                pword = word
                word = 0
            pos = len(hist)
            last = hist[pos - 1]
            if m_len > 0:
                if hist[m_ptr] == last:
                    m_len = min(m_len + 1, 65535)
                elif m_len >= 16:
                    m_len = 1
                else:
                    m_len = 0
                if m_len > 0:
                    m_ptr += 1
            if pos >= 6:
                a = hist[pos - 4] | (hist[pos - 3] << 8) | (hist[pos - 2] << 16) | (hist[pos - 1] << 24)
                bb = hist[pos - 6] | (hist[pos - 5] << 8)
                s = hash2(a, (bb + 0x6D6D6D6D) & M32) >> mshift
                if m_len < 16 and mt[s] > 0:
                    cand = mt[s]
                    l = 0
                    while l < 32 and l < cand and hist[cand - 1 - l] == hist[pos - 1 - l]:
                        l += 1
                    if l >= 6 and l > m_len:
                        m_len = l
                        m_ptr = cand
                mt[s] = pos
            lq = m_len if m_len < 16 else min(16 + ((m_len - 16) >> 2), 31)
            c0, nib, bitpos = 1, 1, 0
            pick_buckets(make_ctx(), c0)
        elif bitpos == 4:
            nib = 1
            pick_buckets(make_ctx(), c0)
    return bytes(out), sp, srclen


def brain_decode_fast(payload, raw_len, codec=3):
    """Same arithmetic as brain_decode_ref, restructured for CPython speed (list comps, locals).
    Cross-checked byte-for-byte against brain_decode_ref. codec=4 selects brain-fast (§6.5).
    P = 1: priming as in brain_decode_ref (§6.4.11); hist (here: out) starts with D."""
    tb, primed, models, mask, src = brain_header(payload, codec)
    light = codec == 4
    n_src = len(src)
    if raw_len == 0:
        return b"", 0, n_src
    D = priming_data() if primed else b""
    nprime = len(D)
    end = nprime + raw_len                            # len(hist) once the block is complete
    pleft = 8 * nprime                                # priming bits still to learn from (§6.4.11)

    nm = len(models)                                  # context models (§6.4.5 / §6.5 mask)
    ni = nm + 3                                       # mixer inputs: 12 (codec 3) or n+3 (codec 4)
    tables, shifts = [], []                           # by position k; models[k] = model index i
    for i in models:
        bits = min(tb, 18) if i == 0 else tb          # §6.4.5: model 0 capped at 18
        tables.append([0] * (1 << bits))
        shifts.append(32 - (bits - 5))
    SM = [[0] * 8192 for _ in range(nm)]
    t0 = [0] * 256
    mt = [0] * (1 << (tb - 3))
    mshift = 32 - (tb - 3)
    m_ptr = m_len = lq = 0
    m_sm = [0] * 64
    WA = [[16384] * ni for _ in range(256)]
    WB = None if light else [[16384] * ni for _ in range(256)]   # §6.5: no mixer B
    WC = [[16384] * ni for _ in range(256)]
    row = [squash((j - 16) * 128) * 16 for j in range(33)]
    apm1 = row * 256
    apm2 = None if light else row * 65536                         # §6.5: no APM2
    S = STRETCH
    SQ = SQTAB
    RC = RECIP
    Z16 = [0] * 16
    zipped_ts = list(zip(range(nm), models, tables, shifts))

    def pick(ctx, c0, base):
        for k, i, tbl, sh in zipped_ts:               # §6.5: model i keeps its own index i
            h = hash2((ctx[i] + (i << 28)) & M32, c0)
            ln = (h >> sh) * 32
            chk = (((h * 0x2545F491) & M32) >> 16) | 1
            if tbl[ln] == chk:
                base[k] = ln
            elif tbl[ln + 16] == chk:
                base[k] = ln + 16
            else:
                b = ln if TOTAL[tbl[ln + 1] & 0x1FFF] < TOTAL[tbl[ln + 17] & 0x1FFF] else ln + 16
                tbl[b:b + 16] = Z16
                tbl[b] = chk
                base[k] = b

    c0 = nib = 1
    bitpos = c4 = c8 = word = pword = 0
    out = bytearray()          # == hist: D first when primed (§6.4.11), then the decoded bytes
    base = [0] * nm
    ctx = all_ctx(0, 0, 0, 0)  # §6.4.9: ctx values computed once before the first byte
    pick(ctx, 1, base)

    x1, x2 = 0, M32
    x = int.from_bytes((bytes(src[:4]) + b"\0\0\0\0")[:4], "big")
    sp = 4

    while True:
        # ---- predict (§6.4.7)
        idx = [b + nib for b in base]
        sts = [t[i] & 0x1FFF for t, i in zip(tables, idx)]
        xs = [S[((sm[s] >> 10) ^ 0x200000) >> 10] for sm, s in zip(SM, sts)]
        xs.append(S[((t0[c0] >> 10) ^ 0x200000) >> 10])
        mctx = -1
        x10 = 0
        if m_len:
            pb = out[m_ptr] | 0x100
            sh = 8 - bitpos
            if (pb >> sh) == c0:
                mctx = lq * 2 + ((pb >> (sh - 1)) & 1)
                x10 = S[((m_sm[mctx] >> 10) ^ 0x200000) >> 10]
        xs.append(x10)             # match input: x[10] (codec 3) / x[n+1] (codec 4)
        xs.append(256)
        c1 = c4 & 0xFF
        wa = WA[c0]
        wc = WC[c1]
        sa = sum(map(mul, xs, wa)) >> 16
        sa = 2047 if sa > 2047 else (-2047 if sa < -2047 else sa)
        sc = sum(map(mul, xs, wc)) >> 16
        sc = 2047 if sc > 2047 else (-2047 if sc < -2047 else sc)
        if light:
            st = (sa + sc) >> 1                       # §6.5: mixers A and C only
        else:
            wb = WB[(lq if mctx >= 0 else 0) * 8 + bitpos]
            sb = sum(map(mul, xs, wb)) >> 16
            sb = 2047 if sb > 2047 else (-2047 if sb < -2047 else sb)
            st = ((sa + sb + sc) * 21846) >> 16
        s2 = st + 2048
        pm = SQ[s2]
        lo = s2 >> 7
        w = s2 & 127
        wn = 128 - w
        k1 = c0 * 33 + lo
        a1 = (apm1[k1] * wn + apm1[k1 + 1] * w) >> 11
        j = w >> 6
        if light:
            p = (pm + 3 * a1 + 2) >> 2                # §6.5: APM1 only
        else:
            k2 = (c0 | (c1 << 8)) * 33 + lo
            a2 = (apm2[k2] * wn + apm2[k2 + 1] * w) >> 11
            p = (pm + a1 + 2 * a2 + 2) >> 2
        p = 1 if p < 1 else (4095 if p > 4095 else p)

        if pleft:
            # ---- priming (§6.4.11): y = next bit of D, most significant first; nothing is coded
            y = (D[len(out)] >> (7 - bitpos)) & 1
            pleft -= 1
        else:
            # ---- arithmetic decode (§6.4.10)
            r = x2 - x1
            xm = x1 + (r >> 12) * p + (((r & 0xFFF) * p) >> 12)
            if x <= xm:
                y = 1
                x2 = xm
            else:
                y = 0
                x1 = xm + 1
            while not ((x1 ^ x2) & 0xFF000000):
                x1 = (x1 << 8) & M32
                x2 = ((x2 << 8) & M32) | 255
                x = ((x << 8) & M32) | (src[sp] if sp < n_src else 0)
                sp += 1

        # ---- learn (§6.4.8)
        if y:
            for sm, s in zip(SM, sts):
                c = sm[s]
                n = c & 1023
                q = (c >> 10) ^ 0x200000
                q += ((4194303 - q) * RC[n]) >> 16
                sm[s] = ((q ^ 0x200000) << 10) | (n + 1 if n < 1023 else n)
            for t, i in zip(tables, idx):
                t[i] = NEXT1[t[i] & 0x1FFF]
            c = t0[c0]
            n = c & 1023
            q = (c >> 10) ^ 0x200000
            q += ((4194303 - q) * RC[n]) >> 16
            t0[c0] = ((q ^ 0x200000) << 10) | (n + 1 if n < 60 else n)
            if mctx >= 0:
                c = m_sm[mctx]
                n = c & 1023
                q = (c >> 10) ^ 0x200000
                q += ((4194303 - q) * RC[n]) >> 16
                m_sm[mctx] = ((q ^ 0x200000) << 10) | (n + 1 if n < 1023 else n)
            g = 65662
        else:
            for sm, s in zip(SM, sts):
                c = sm[s]
                n = c & 1023
                q = (c >> 10) ^ 0x200000
                q += (-q * RC[n]) >> 16
                sm[s] = ((q ^ 0x200000) << 10) | (n + 1 if n < 1023 else n)
            for t, i in zip(tables, idx):
                t[i] = NEXT0[t[i] & 0x1FFF]
            c = t0[c0]
            n = c & 1023
            q = (c >> 10) ^ 0x200000
            q += (-q * RC[n]) >> 16
            t0[c0] = ((q ^ 0x200000) << 10) | (n + 1 if n < 60 else n)
            if mctx >= 0:
                c = m_sm[mctx]
                n = c & 1023
                q = (c >> 10) ^ 0x200000
                q += (-q * RC[n]) >> 16
                m_sm[mctx] = ((q ^ 0x200000) << 10) | (n + 1 if n < 1023 else n)
            g = 0
        yy = y << 12
        e = (yy - SQ[sa + 2048]) * 16
        wa[:] = [v + ((xi * e) >> 16) for v, xi in zip(wa, xs)]
        if max(wa) > I32MAX or min(wa) < I32MIN:
            wa[:] = [wrap32(v) for v in wa]
        if not light:
            e = (yy - SQ[sb + 2048]) * 16
            wb[:] = [v + ((xi * e) >> 16) for v, xi in zip(wb, xs)]
            if max(wb) > I32MAX or min(wb) < I32MIN:
                wb[:] = [wrap32(v) for v in wb]
        e = (yy - SQ[sc + 2048]) * 16
        wc[:] = [v + ((xi * e) >> 16) for v, xi in zip(wc, xs)]
        if max(wc) > I32MAX or min(wc) < I32MIN:
            wc[:] = [wrap32(v) for v in wc]
        k1 += j
        apm1[k1] += (g - apm1[k1]) >> 7
        if not light:
            k2 += j
            apm2[k2] += (g - apm2[k2]) >> 7
        c0 = c0 * 2 + y
        nib = nib * 2 + y
        bitpos += 1

        if bitpos == 8:
            # ---- byte boundary (§6.4.9)
            c = c0 & 0xFF
            out.append(c)
            pos = len(out)
            if pos == end:
                break
            if pos > nprime and not ((pos - nprime) & 0xFFFF):
                print(f"      ... brain progress {pos - nprime}/{raw_len} bytes", file=sys.stderr, flush=True)
            c8 = ((c8 << 8) | (c4 >> 24)) & M32
            c4 = ((c4 << 8) | c) & M32
            lc = c + 32 if 65 <= c <= 90 else c
            if 97 <= lc <= 122 or c >= 128:
                word = hash2((word + lc) & M32, 0x77770001)
            elif word:
                pword = word
                word = 0
            # match model (§6.4.6)
            if m_len:
                if out[m_ptr] == c:
                    m_len = m_len + 1 if m_len < 65535 else 65535
                elif m_len >= 16:
                    m_len = 1
                else:
                    m_len = 0
                if m_len:
                    m_ptr += 1
            if pos >= 6:
                s = hash2(out[pos - 4] | (out[pos - 3] << 8) | (out[pos - 2] << 16) | (c << 24),
                          ((out[pos - 6] | (out[pos - 5] << 8)) + 0x6D6D6D6D) & M32) >> mshift
                if m_len < 16:
                    cand = mt[s]
                    if cand:
                        l = 0
                        while l < 32 and l < cand and out[cand - 1 - l] == out[pos - 1 - l]:
                            l += 1
                        if l >= 6 and l > m_len:
                            m_len = l
                            m_ptr = cand
                mt[s] = pos
            lq = m_len if m_len < 16 else min(16 + ((m_len - 16) >> 2), 31)
            c0 = nib = 1
            bitpos = 0
            ctx = all_ctx(c4, c8, word, pword)        # pick() reads only the models in use
            pick(ctx, 1, base)
        elif bitpos == 4:
            nib = 1
            pick(ctx, c0, base)           # same ctx_i as at the byte boundary (§6.4.5)
    return bytes(out[nprime:]), sp, n_src


BRAIN_IMPL = {"fast": brain_decode_fast, "ref": brain_decode_ref}
OPTS = {"brain": "fast", "prime": PRIME_DEFAULT}


# ---------------------------------------------------------------- codecs (§6)
def decompress(codec, payload, raw_len, what, index=False):
    if index:
        need(codec in (0, 1, 2), f"{what}: index frame codec {codec} not allowed")
    if codec == 0:
        need(len(payload) == raw_len, f"{what}: store payload length != raw_len")
        return bytes(payload)
    if codec == 1:
        d = zstandard.ZstdDecompressor(max_window_size=1 << 28)
        r = d.stream_reader(bytes(payload), read_across_frames=True)
        parts, total = [], 0
        while total <= raw_len:
            piece = r.read(1 << 20)
            if not piece:
                break
            parts.append(piece)
            total += len(piece)
        out = b"".join(parts)
        need(len(out) == raw_len, f"{what}: zstd output {len(out)} != raw_len {raw_len}")
        return out
    if codec == 2:
        need(len(payload) >= 1, f"{what}: lzma2 empty")
        pbyte = payload[0]
        need(pbyte <= 32, f"{what}: lzma2 dict prop {pbyte} > 32")
        dsz = (2 | (pbyte & 1)) << (pbyte // 2 + 11)
        dec = lzma.LZMADecompressor(format=lzma.FORMAT_RAW,
                                    filters=[{"id": lzma.FILTER_LZMA2, "dict_size": dsz}])
        out = dec.decompress(bytes(payload[1:]), max_length=raw_len + 1)
        need(dec.eof, f"{what}: lzma2 no end marker")
        need(dec.unused_data == b"", f"{what}: bytes after lzma2 end marker")
        need(len(out) == raw_len, f"{what}: lzma2 output length mismatch")
        log(f"    lzma2: prop={pbyte} dict={dsz} bytes")
        return out
    if codec in (3, 4):             # brain (§6.4) / brain-fast (§6.5); data blocks only
        name = "brain" if codec == 3 else "brain-fast"
        out, used, avail = BRAIN_IMPL[OPTS["brain"]](payload, raw_len, codec)
        tb, primed, models, mask, _ = brain_header(payload, codec)
        mtxt = "" if mask is None else f" mask={mask:#06x} models={models}"
        ptxt = f" P=1 (primed with {PRIME_V1_LEN} bytes)" if primed else " P=0"
        log(f"    {name}[{OPTS['brain']}]: tb={tb}{ptxt}{mtxt} consumed {min(used, avail)}/{avail} AC bytes "
            f"(+{max(0, used - avail)} implicit zero bytes)")
        need(len(out) == raw_len, f"{what}: {name} length mismatch")
        return out
    raise FormatError(f"{what}: unknown codec {codec}")


def parse_frame(buf, off, magic, what):
    need(off + 16 <= len(buf), f"{what}: frame header out of range")
    hdr = buf[off:off + 16]
    need(hdr[0:4] == magic, f"{what}: bad frame magic {hdr[0:4]!r} (want {magic!r})")
    codec, flags = hdr[4], hdr[5]
    reserved, raw_len, stored_len = struct.unpack("<HII", hdr[6:16])
    need(reserved == 0, f"{what}: reserved != 0")
    need(flags & ~1 == 0, f"{what}: unknown frame flags {flags:#x}")
    need(off + 16 + stored_len <= len(buf), f"{what}: payload out of range")
    payload = buf[off + 16:off + 16 + stored_len]
    return dict(hdr=hdr, codec=codec, enc=bool(flags & 1), raw_len=raw_len,
                stored_len=stored_len, payload=payload, end=off + 16 + stored_len)


# ---------------------------------------------------------------- encryption (§4.1, §13)
def parse_enc_record(val):
    cipher, kdf = val[0], val[1]
    need(cipher == 1, f"unknown cipher {cipher}")
    need(kdf == 1, f"unknown kdf {kdf}")
    salt = val[2:18]
    m_kib, t_cost, p_lanes = struct.unpack("<III", val[18:30])
    need(1 <= p_lanes <= 16, f"KDF p={p_lanes} out of range")
    need(1 <= t_cost <= 64, f"KDF t={t_cost} out of range")
    need(8 * p_lanes <= m_kib <= 4194304, f"KDF m_kib={m_kib} out of range")
    return dict(salt=salt, m_kib=m_kib, t_cost=t_cost, p_lanes=p_lanes, extra=len(val) - 30)


def derive_key(password, rec):
    from argon2.low_level import hash_secret_raw, Type
    master = hash_secret_raw(secret=password, salt=rec["salt"], time_cost=rec["t_cost"],
                             memory_cost=rec["m_kib"], parallelism=rec["p_lanes"],
                             hash_len=32, type=Type.ID, version=19)
    return blake3.blake3(master, derive_key_context="ezpz v1 data encryption key").digest()


def aead_open(key, archive_id, counter, hdr16, payload, what):
    import nacl.bindings
    import nacl.exceptions
    need(len(payload) >= 16, f"{what}: encrypted payload shorter than tag")
    nonce = archive_id + struct.pack("<Q", counter)
    try:
        return nacl.bindings.crypto_aead_xchacha20poly1305_ietf_decrypt(bytes(payload), bytes(hdr16), nonce, key)
    except nacl.exceptions.CryptoError:
        raise FormatError(f"{what}: AEAD decryption failed (wrong password or tampered)")


# ---------------------------------------------------------------- transforms (§9)
def x86(buf, encode):
    buf = bytearray(buf)
    i = 0
    n = len(buf)
    cnt = 0
    while i + 5 <= n:
        if buf[i] == 0xE8 or buf[i] == 0xE9:
            v = struct.unpack_from("<I", buf, i + 1)[0]
            if (v >> 23) == 0 or (v >> 23) == 0x1FF:
                r = ((v + i) if encode else (v - i)) & 0x00FFFFFF
                if r & 0x00800000:
                    r |= 0xFF000000
                struct.pack_into("<I", buf, i + 1, r)
                cnt += 1
            i += 5
        else:
            i += 1
    return bytes(buf), cnt


def arm64(buf, encode):
    buf = bytearray(buf)
    i = 0
    cnt = 0
    while i + 4 <= len(buf):
        w = struct.unpack_from("<I", buf, i)[0]
        if (w & 0xFC000000) == 0x94000000:
            imm = w & 0x03FFFFFF
            pc = i >> 2
            imm = ((imm + pc) if encode else (imm - pc)) & 0x03FFFFFF
            struct.pack_into("<I", buf, i, 0x94000000 | imm)
            cnt += 1
        i += 4
    return bytes(buf), cnt


# §9 transform 3: byte-value sets copied from the spec text (hexadecimal)
OP1_X64 = frozenset(bytes.fromhex(
    "01 03 09 0B 21 23 29 2B 31 33 38 39 3A 3B 63 80 81 83 84 85 88 89 8A 8B 8D C6 C7 F7 FF"))
OP2_X64 = frozenset(bytes.fromhex(
    "10 11 12 13 16 17 28 29 2A 2E 2F 51 54 57 58 59 5A 5C 5E 6F 74 76 7F B6 B7 BE BF D6 DB EB EF"))


def conv24(buf, off, pos, encode):
    """§9 conv24: the 24-bit rule of transform 1 for the u32le field at off. Returns 1 if rewritten."""
    v = buf[off] | (buf[off + 1] << 8) | (buf[off + 2] << 16) | (buf[off + 3] << 24)
    if (v >> 23) == 0 or (v >> 23) == 0x1FF:
        r = ((v + pos) if encode else (v - pos)) & 0x00FFFFFF
        if r & 0x00800000:
            r |= 0xFF000000
        struct.pack_into("<I", buf, off, r)
        return 1
    return 0


def x86_64(buf, encode):
    """§9 transform 3 (x86-64: E8/E9 plus RIP-relative disp32 after a mod=00 r/m=101 ModRM).
    Transcribed from the pseudocode: a matched field is skipped whether or not conv24 rewrote it,
    and conv24 gets pos = i (where the walk stood, i.e. the legacy prefix if there is one)."""
    buf = bytearray(buf)
    n = len(buf)
    i = 0
    calls = rips = 0
    while i + 5 <= n:
        c = buf[i]
        if c == 0xE8 or c == 0xE9:
            calls += conv24(buf, i + 1, i, encode)
            i += 5
            continue
        j = i
        if c == 0x66 or c == 0xF2 or c == 0xF3:       # one legacy prefix
            j += 1
        if j < n and (buf[j] & 0xF0) == 0x40:         # REX prefix
            j += 1
        if j + 7 <= n and buf[j] == 0x0F and buf[j + 1] in OP2_X64 and (buf[j + 2] & 0xC7) == 0x05:
            rips += conv24(buf, j + 3, i, encode)
            i = j + 7
            continue
        if j + 6 <= n and buf[j] in OP1_X64 and (buf[j + 1] & 0xC7) == 0x05:
            rips += conv24(buf, j + 2, i, encode)
            i = j + 6
            continue
        i += 1
    return bytes(buf), calls, rips


TRANSFORMS = (0, 1, 2, 3)          # §9 table


# ---------------------------------------------------------------- path rules (§8)
def utf8(b, what):
    try:
        return b.decode("utf-8")
    except UnicodeDecodeError:
        raise FormatError(f"{what} is not valid UTF-8")


def check_path(p):
    need(1 <= len(p) <= 4096, f"path length {len(p)}")
    try:
        s = p.decode("utf-8")
    except UnicodeDecodeError:
        raise FormatError(f"path not UTF-8: {p!r}")
    need(not s.startswith("/") and not s.endswith("/"), f"path starts/ends with '/': {s}")
    need("\0" not in s and "\\" not in s, f"path has NUL/backslash: {s!r}")
    comps = s.split("/")
    for c in comps:
        need(c not in ("", ".", ".."), f"bad path component in {s!r}")
    need(":" not in comps[0], f"colon in first path component: {s!r}")
    return s


# ---------------------------------------------------------------- main reader
def read_archive(path, pubhex=None, chunkref_mode="global", password=None):
    buf = open(path, "rb").read()
    L = len(buf)
    log(f"== {path} ({L} bytes)")
    need(L >= 96, "file shorter than 96 bytes")

    # §3 header
    r = Rd(buf, "header")
    need(r.take(8) == b"\x89EZPZ\r\n\x1a", "bad magic")
    vmaj, vmin, hflags, ext_len = r.u8(), r.u8(), r.u16(), r.u32()
    archive_id = r.take(16)
    need(vmaj == 1, f"version_major {vmaj}")
    need(hflags & ~3 == 0, f"unknown header flags {hflags:#x}")
    need(ext_len <= 65536, "ext_len > 65536")
    hdr_len = 32 + ext_len
    need(hdr_len + 64 <= L, "header extension runs past trailer")
    log(f"  header: v{vmaj}.{vmin} flags={hflags:#x} (enc={hflags & 1} sig={(hflags >> 1) & 1}) "
        f"ext_len={ext_len} archive_id={archive_id.hex()}")
    # §4 extension records
    er = Rd(buf[32:hdr_len], "ext")
    seen = set()
    enc_rec = None
    while not er.eof():
        t = er.u8()
        ln = er.u16()
        val = er.take(ln)
        need(t not in seen, f"duplicate ext record {t:#x}")
        seen.add(t)
        if t == 0x81:
            need(ln >= 30, "encryption record too short")
            enc_rec = parse_enc_record(val)
            log(f"  ext 0x81 encryption: XChaCha20-Poly1305 / Argon2id m={enc_rec['m_kib']} KiB "
                f"t={enc_rec['t_cost']} p={enc_rec['p_lanes']} salt={enc_rec['salt'].hex()} extra={enc_rec['extra']}")
        elif t & 0x80:
            raise FormatError(f"unknown critical ext record {t:#x}")
        else:
            log(f"  skipping unknown ext record {t:#x} ({ln} bytes)")
    need(bool(hflags & 1) == (enc_rec is not None), "encryption flag/record mismatch")
    encrypted = bool(hflags & 1)
    signed = bool(hflags & 2)

    # §11 trailer
    tr = buf[L - 64:]
    need(tr[56:64] == b"EZPZEND\x1a", "bad end magic")
    need(b3(tr[:52])[:4] == tr[52:56], "trailer check mismatch")
    index_offset, archive_len = struct.unpack("<QQ", tr[0:16])
    root_hash = tr[16:48]
    tflags = struct.unpack("<I", tr[48:52])[0]
    need(archive_len == L, f"archive_len {archive_len} != file length {L}")
    need(tflags & ~1 == 0, f"unknown trailer flags {tflags:#x}")
    need(bool(tflags & 1) == signed, "trailer/header signature flag mismatch")
    log(f"  trailer: index_offset={index_offset} archive_len={archive_len} flags={tflags} "
        f"check OK, end magic OK, root_hash={root_hash.hex()}")

    idx_end = L - 64 - (104 if signed else 0)
    need(hdr_len <= index_offset and index_offset + 32 <= idx_end, "index_offset out of range (§11)")
    index_region = buf[index_offset:idx_end]

    # §12 root hash
    calc = b3(buf[:hdr_len] + index_region)
    need(calc == root_hash, f"root_hash mismatch: calc {calc.hex()}")
    log("  root_hash = BLAKE3(header || index region): OK")

    # §10 signature
    if pubhex is not None:
        need(signed, "a public key was expected but the archive is unsigned (§10: MUST reject)")
    if signed:
        sg = buf[idx_end:idx_end + 104]
        need(sg[0:4] == b"EZSG", "bad signature magic")
        need(sg[4] == 1, f"unknown signature algo {sg[4]}")
        need(sg[5:8] == b"\0\0\0", "signature reserved != 0")
        pk, sig = sg[8:40], sg[40:104]
        ed25519_verify_strict(pk, sig, b"EZPZ-v1 signature\0" + root_hash)
        log(f"  signature: strict Ed25519 OK, pubkey={pk.hex()}")
        if pubhex is not None:
            need(pk.hex() == pubhex.lower(), f"embedded pubkey != expected {pubhex}")
            log("  signature: embedded public key matches the expected key")

    # §7 block table
    bt = parse_frame(buf, index_offset, b"EZBT", "block table")
    need(not bt["enc"], "block table must not be encrypted")
    need(bt["raw_len"] <= 1 << 30, "block table raw_len > 1 GiB")
    btraw = decompress(bt["codec"], bt["payload"], bt["raw_len"], "block table", index=True)
    log(f"  block table frame: codec={bt['codec']} raw={bt['raw_len']} stored={bt['stored_len']}")
    br = Rd(btraw, "block table")
    need(br.varint() == 1, "block table version != 1")
    n = br.varint()
    blk_raw = [br.varint() for _ in range(n)]
    blk_stored = [br.varint() for _ in range(n)]
    blk_hash = [br.take(32) for _ in range(n)]
    need(br.eof(), "trailing bytes in block table")
    for v in blk_raw:
        need(1 <= v <= 268435456, "block raw_len out of range")
    offs = []
    o = hdr_len
    for k in range(n):
        offs.append(o)
        o += 16 + blk_stored[k]
    need(o == index_offset, f"blocks end at {o}, index_offset {index_offset}")
    log(f"  blocks: n={n} (last block ends exactly at index_offset)")

    # data block frames: header/table agreement + hash (possible without the password, §14)
    frames = []
    for k in range(n):
        fr = parse_frame(buf, offs[k], b"EZBK", f"block {k}")
        need(fr["raw_len"] == blk_raw[k] and fr["stored_len"] == blk_stored[k],
             f"block {k}: frame header sizes disagree with block table")
        need(1 <= fr["raw_len"] <= 268435456, f"block {k}: raw_len range")
        need(fr["enc"] == encrypted, f"block {k}: encryption bit != header")
        need(b3(fr["hdr"] + fr["payload"]) == blk_hash[k], f"block {k}: BLAKE3 mismatch")
        frames.append(fr)
        log(f"  block {k}: off={offs[k]} codec={fr['codec']} enc={int(fr['enc'])} raw={fr['raw_len']} "
            f"stored={fr['stored_len']} hash OK")

    # §8 catalog frame (§2, §14 step 8)
    ct = parse_frame(buf, bt["end"], b"EZCT", "catalog")
    need(ct["end"] == idx_end, f"catalog ends at {ct['end']}, index region ends at {idx_end}")
    need(ct["enc"] == encrypted, "catalog encryption bit != header")
    need(ct["codec"] in (0, 1, 2), "catalog codec not in {0,1,2}")
    need(ct["raw_len"] <= 1 << 30, "catalog raw_len > 1 GiB")
    log(f"  catalog frame: codec={ct['codec']} enc={int(ct['enc'])} raw={ct['raw_len']} stored={ct['stored_len']}")
    key = None
    if encrypted:
        if password is None:
            log("  encrypted and no password: verified header, trailer, root_hash, block table and ALL block hashes")
            return None
        key = derive_key(password.encode("utf-8"), enc_rec)
        ctplain = aead_open(key, archive_id, 2 ** 64 - 1, ct["hdr"], ct["payload"], "catalog")
        log(f"  catalog: AEAD OK (counter 2^64-1, aad = catalog frame header), {len(ctplain)} compressed bytes")
    else:
        ctplain = ct["payload"]
    ctraw = decompress(ct["codec"], ctplain, ct["raw_len"], "catalog", index=True)
    cr = Rd(ctraw, "catalog")
    need(cr.varint() == 1, "catalog version != 1")
    hash_len = cr.u8()
    need(hash_len in (0, 16, 32), f"hash_len {hash_len}")
    created = cr.svarint()
    creator = utf8(cr.take(cr.varint()), "creator")
    block_count = cr.varint()
    need(block_count == n, "catalog block_count != block table n")
    chunk_count = [cr.varint() for _ in range(block_count)]
    chunks = []  # (block, offset, length)
    for b in range(block_count):
        off = 0
        for _ in range(chunk_count[b]):
            cl = cr.varint()
            need(cl >= 1, "chunk_len 0")
            chunks.append((b, off, cl))
            off += cl
        need(off == blk_raw[b], f"block {b}: chunk_len sum {off} != raw_len {blk_raw[b]}")
    m = cr.varint()
    paths = []
    prev = b""
    for _ in range(m):
        shared = cr.varint()
        need(shared <= len(prev), "front-coding shared > previous length")
        suffix = cr.take(cr.varint())
        p = prev[:shared] + suffix
        paths.append(p)
        prev = p
    types = [cr.u8() for _ in range(m)]
    modes_raw = [cr.varint() for _ in range(m)]
    mtimes = []
    t = 0
    for _ in range(m):
        t += cr.svarint()
        mtimes.append(t)
    nsecs = [cr.varint() for _ in range(m)]
    for i in range(m):
        need(types[i] in (0, 1, 2), f"bad entry type {types[i]}")
        need(nsecs[i] <= 999999999, "mtime_nsec out of range")
    highbits = sum(1 for v in modes_raw if v & ~0o7777)
    modes = [v & 0o7777 for v in modes_raw]          # §8: ignore (mask) bits outside 0o7777
    spaths = [check_path(p) for p in paths]
    for i in range(1, m):
        need(paths[i - 1] < paths[i], f"entries not strictly sorted: {paths[i-1]!r} !< {paths[i]!r}")
    nondir = {paths[i] for i in range(m) if types[i] != 1}
    for p in paths:                                  # §8 nesting ban
        comps = p.split(b"/")
        for k in range(1, len(comps)):
            anc = b"/".join(comps[:k])
            need(anc not in nondir, f"entry {p!r} lies under non-directory entry {anc!r}")
    files = [i for i in range(m) if types[i] == 0]
    links = [i for i in range(m) if types[i] == 2]
    f = len(files)
    transforms = [cr.u8() for _ in range(f)]
    for tf in transforms:                            # §9 ids 0..3; anything else is not understood (§17)
        need(tf in TRANSFORMS, f"unknown transform id {tf}")
    nchunks = [cr.varint() for _ in range(f)]
    refs = []
    prevref = -1
    for j in range(f):
        if chunkref_mode == "perfile":
            prevref = -1
        rl = []
        for _ in range(nchunks[j]):
            ref = cr.svarint() + prevref + 1
            need(0 <= ref < len(chunks), f"chunk ref {ref} out of range (0..{len(chunks)-1})")
            rl.append(ref)
            prevref = ref
        refs.append(rl)
    fhash = [cr.take(hash_len) for _ in range(f)]
    targets = {}
    for i in links:
        targets[i] = utf8(cr.take(cr.varint()), "symlink target")
    need(cr.eof(), f"trailing bytes in catalog ({len(ctraw) - cr.p})")
    if br.nonminimal or cr.nonminimal:
        log(f"  NOTE: non-minimal varints seen: blocktable={br.nonminimal} catalog={cr.nonminimal}")
    if highbits:
        log(f"  NOTE: {highbits} mode values had bits outside 0o7777 (masked)")
    used = set(x for rl in refs for x in rl)
    log(f"  catalog: hash_len={hash_len} created={created} creator={creator!r} chunks={len(chunks)} "
        f"(per block {chunk_count}), referenced={len(used)}, entries={m}")
    tn = {0: "file", 1: "dir", 2: "symlink"}
    fi = {e: j for j, e in enumerate(files)}
    for i in range(m):
        extra = ""
        if i in fi:
            j = fi[i]
            size = sum(chunks[c][2] for c in refs[j])
            extra = f" size={size} transform={transforms[j]} chunks={refs[j]} hash={fhash[j].hex() or '-'}"
        if i in targets:
            extra = f" -> {targets[i]!r}"
        log(f"    [{tn[types[i]]:7}] {spaths[i]!r} mode={modes[i]:o} mtime={mtimes[i]}.{nsecs[i]:09d}{extra}")

    # §14 step 9
    blocks = {}

    def get_block(k):
        if k in blocks:
            return blocks[k]
        fr = frames[k]
        payload = fr["payload"]
        if encrypted:
            payload = aead_open(key, archive_id, k, fr["hdr"], payload, f"block {k}")
        data = decompress(fr["codec"], payload, fr["raw_len"], f"block {k}")
        blocks[k] = data
        return data

    result = {}
    for j, i in enumerate(files):
        parts = []
        for ref in refs[j]:
            b, off, ln = chunks[ref]
            parts.append(get_block(b)[off:off + ln])
        content = b"".join(parts)
        tf = transforms[j]
        if tf == 1:
            content, cnt = x86(content, False)
            log(f"    {spaths[i]}: undid x86 transform ({cnt} E8/E9 operands rewritten)")
        elif tf == 2:
            content, cnt = arm64(content, False)
            log(f"    {spaths[i]}: undid ARM64 transform ({cnt} BL words rewritten)")
        elif tf == 3:
            content, calls, rips = x86_64(content, False)
            log(f"    {spaths[i]}: undid x86-64 transform ({calls} E8/E9 operands, "
                f"{rips} RIP-relative displacements rewritten)")
        else:
            need(tf == 0, f"unknown transform {tf}")
        if hash_len:
            need(b3(content)[:hash_len] == fhash[j], f"file hash mismatch: {spaths[i]}")
        result[i] = content
    for k in range(n):
        if k not in blocks:
            log(f"  NOTE: block {k} not referenced by any file")
    log(f"  all {f} file hashes OK (hash_len={hash_len})" if hash_len else
        "  hash_len=0: no file hashes (integrity from block hashes + AEAD)")
    entries = []
    for i in range(m):
        entries.append(dict(path=spaths[i], type=types[i], mode=modes[i], mtime=mtimes[i],
                            nsec=nsecs[i], data=result.get(i), target=targets.get(i)))
    return entries


def extract(entries, outdir):
    """§8/§16 extraction: create missing parents, files, then symlinks, then directory mode/mtime
    deepest-first; never restore setuid/setgid/sticky."""
    os.makedirs(outdir, exist_ok=True)
    real = os.path.realpath(outdir)

    def dst_of(e):
        d = os.path.join(outdir, e["path"])
        need(os.path.realpath(os.path.dirname(d)).startswith(real), "zip-slip")
        return d

    for e in entries:
        if e["type"] == 1:
            os.makedirs(dst_of(e), exist_ok=True)
    for e in entries:
        if e["type"] == 0:
            dst = dst_of(e)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            if os.path.lexists(dst):
                os.remove(dst)
            with open(dst, "wb") as fh:
                fh.write(e["data"])
            os.chmod(dst, e["mode"] & 0o777)
            os.utime(dst, ns=(e["mtime"] * 10**9 + e["nsec"],) * 2)
    for e in entries:
        if e["type"] == 2:
            dst = dst_of(e)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            if os.path.lexists(dst):
                os.remove(dst)
            os.symlink(e["target"], dst)
            os.utime(dst, ns=(e["mtime"] * 10**9 + e["nsec"],) * 2, follow_symlinks=False)
    for e in sorted((e for e in entries if e["type"] == 1), key=lambda e: -e["path"].count("/")):
        dst = dst_of(e)
        os.chmod(dst, e["mode"] & 0o777)
        os.utime(dst, ns=(e["mtime"] * 10**9 + e["nsec"],) * 2)


def compare(a, b, strict=False):
    """Compare tree a (original) with tree b (extracted), root included.
    Content, entry types and symlink targets always; modes and mtimes only with strict=True
    (a git checkout or unzip does not keep those, so they would differ for reasons
    unrelated to the archive)."""
    probs = []

    def walk(root):
        res = {".": root}
        for dp, dns, fns in os.walk(root):
            for nme in dns + fns:
                full = os.path.join(dp, nme)
                res[os.path.relpath(full, root)] = full
        return res
    ta, tb = walk(a), walk(b)
    for k in sorted(set(ta) | set(tb)):
        if k not in ta or k not in tb:
            probs.append(f"{k}: only in {'original' if k in ta else 'extracted'}")
            continue
        pa, pb = ta[k], tb[k]
        sa, sb = os.lstat(pa), os.lstat(pb)
        if os.path.islink(pa) or os.path.islink(pb):
            if not (os.path.islink(pa) and os.path.islink(pb)) or os.readlink(pa) != os.readlink(pb):
                probs.append(f"{k}: symlink differs")
        elif os.path.isdir(pa) != os.path.isdir(pb):
            probs.append(f"{k}: type differs")
        elif os.path.isfile(pa):
            if open(pa, "rb").read() != open(pb, "rb").read():
                probs.append(f"{k}: content differs")
        if strict and (sa.st_mode & 0o7777) != (sb.st_mode & 0o7777) and not os.path.islink(pa):
            probs.append(f"{k}: mode {sa.st_mode & 0o7777:o} vs {sb.st_mode & 0o7777:o}")
        # symlink mtimes are skipped: many unpackers (e.g. unzip) cannot restore them on the original side
        if strict and sa.st_mtime_ns != sb.st_mtime_ns and not os.path.islink(pa):
            probs.append(f"{k}: mtime {sa.st_mtime_ns} vs {sb.st_mtime_ns}")
    return probs, len(ta)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("archive")
    ap.add_argument("--out")
    ap.add_argument("--pub", help="file with the expected Ed25519 public key (hex)")
    ap.add_argument("--password")
    ap.add_argument("--compare")
    ap.add_argument("--strict", action="store_true", help="also compare modes and mtimes")
    ap.add_argument("--sub", default="", help="compare --compare against OUT/SUB")
    ap.add_argument("--chunkref", default="global", choices=["global", "perfile"])
    ap.add_argument("--brain-impl", default="fast", choices=["fast", "ref"])
    ap.add_argument("--prime", default=PRIME_DEFAULT,
                    help="priming data file for P=1 brain blocks (Appendix B; default: ../prime/v1.txt "
                         "relative to this script). Size and BLAKE3 are checked before use.")
    a = ap.parse_args()
    OPTS["brain"] = a.brain_impl
    OPTS["prime"] = a.prime
    pub = open(a.pub).read().strip() if a.pub else None
    try:
        entries = read_archive(a.archive, pub, a.chunkref, a.password)
        if entries is None:
            log("RESULT: PASS (keyless verification only)")
            return 0
        if a.out:
            extract(entries, a.out)
            log(f"  extracted to {a.out}")
        if a.compare and a.out:
            probs, cnt = compare(a.compare, os.path.join(a.out, a.sub), a.strict)
            if probs:
                for p in probs:
                    log("  DIFF:", p)
                log("RESULT: FAIL (tree differs)")
                return 1
            what = "content, symlink targets, modes, mtimes" if a.strict else "content and symlink targets"
            log(f"  tree compare vs {a.compare}: {cnt} entries identical ({what})")
        log("RESULT: PASS")
        return 0
    except FormatError as ex:
        log("RESULT: FAIL:", ex)
        return 1


if __name__ == "__main__":
    sys.exit(main())
