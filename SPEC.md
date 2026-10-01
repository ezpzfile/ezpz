# EZPZ archive format specification v1.0 (draft)

English | [한국어](SPEC.ko.md)

- File extension: `.ezpz`
- Media type (proposed): `application/x-ezpz`
- Status: draft. The reference implementation is `ezpz` (Rust) in this repository.
- Date: 2026-10-02

---

## 0. Overview

`.ezpz` is an archive format: it packs files and folders into one file and compresses them, like zip or 7z. It brings together techniques that grew up separately in other formats and adds a codec of its own, the brain codec.

| Feature | zip | 7z | tar.zst | **ezpz** |
|---|---|---|---|---|
| Compresses files together (solid) | ✗ | ✓ | ✓ | ✓ (per block) |
| Fast extraction of a single file | ✓ | △ (per block) | ✗ (decompresses from the start) | ✓ (reads only the blocks it needs) |
| Stores identical content once (deduplication) | ✗ | △¹ | △¹ | ✓ (content-defined chunks, any distance) |
| Skips already-compressed files (jpg, mp4, ...) | ✗ | △ | ✗ | ✓ (detected automatically) |
| Preprocessing for executables | ✗ | ✓ | ✗ | ✓ (x86, ARM64) |
| Multi-core compression and extraction | ✗ | △ | ✓ | ✓ |
| Encryption that also hides file names | ✗ | ✓ | ✗ | ✓ |
| Damage check on an encrypted archive without the password (tamper check too when signed) | ✗ | ✗ | ✗ | ✓ |
| Digital signature (proof that a key holder approved this content) | ✗ | ✗ | ✗ | ✓ (Ed25519) |
| Prediction codec that learns as it runs | ✗ | ✗ | ✗ | ✓ (brain codecs, for maximum compression) |

¹ Repeated content shrinks only when it falls inside the compressor's memory (its dictionary or window, typically tens to hundreds of MiB).

### In plain words

- **Blocks.** Files are sorted so that similar ones sit next to each other, joined end to end, and cut into blocks of a fixed size (for example 32 MiB) that are compressed separately. Joining them lets the compressor use similarities between files (solid compression). Cutting them into blocks means that extracting one file only requires decompressing the blocks that contain it.
- **Deduplication.** File contents are split at boundaries chosen by the content itself, so identical content becomes identical chunks wherever it appears, and each chunk is stored once. This pays off most when an archive holds several versions of the same project.
- **Integrity chain.** The trailer at the end holds a hash of the header and the index, and the index holds a hash of every block, so accidental damage to even one byte is always detected. Anyone can recompute these hashes, though. Detecting deliberate changes needs a signature, or a copy of the hash kept somewhere trusted. The signature sits at the top of the chain.
- **Brain codec.** It predicts each bit before coding it and stores only how far the prediction was off. Several small predictors each give an estimate, and a small neural network learns, as it goes, which of them to trust in the current situation. The compressor and the decompressor learn in exactly the same way, like twins, so the learned model never has to be stored in the file. A lighter version, brain-fast, runs fewer predictors: it is 1.2 to 2 times as fast, depending on the processor, and its output is 1.5 to 8% larger, depending on the data.

---

## 1. Conventions

- **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used as defined in RFC 2119.
- All fixed-size integers are little-endian. One exception: the arithmetic-coded bit stream of the brain codecs (codecs 3 and 4) is read and written most significant byte first (big-endian), see §6.4.10.
- `u8/u16/u32/u64` are unsigned integers; `i32/i64` are two's-complement signed integers.
- `>>` on signed values is an arithmetic shift (it rounds down). `/` is integer division that truncates toward zero.
- **varint**: unsigned LEB128, at most 64 bits and at most 10 bytes. Seven bits per byte, low bits first, with the high bit set when more bytes follow. Encoders MUST write the shortest form. Decoders MUST reject non-minimal forms (a varint that ends in an unnecessary 0x00 byte) and values above 64 bits, so that every decoder judges a given file the same way.
- **svarint**: an i64 zigzag-encoded, then written as a varint. Encode `u = (v << 1) ^ (v >> 63)`, decode `v = (u >> 1) ^ -(u & 1)` (all 64-bit).
- **BLAKE3** means BLAKE3-256 with a 32-byte output unless stated otherwise.
- Offsets are byte positions from the start of the file.
- Unless stated otherwise, u32 additions and multiplications (in particular the hash inputs in §6.4) wrap modulo 2^32.

## 2. File layout

```
offset 0
+------------------------------------------+
| header (32 + E bytes)                    |  §3, §4
+------------------------------------------+
| data block frame 0                       |  §5
| data block frame 1                       |
| ... (contiguous, no gaps)                |
+------------------------------------------+  <- index_offset
| block table frame (never encrypted)      |  §7
| catalog frame (may be encrypted)         |  §8
+------------------------------------------+
| signature section (104 bytes, optional)  |  §10
+------------------------------------------+
| trailer (64 bytes)                       |  §11
+------------------------------------------+  end of file
```

- Data blocks MUST follow the header directly, with no gaps between them. The offset of block `k` is `header_length + Σ_{i<k} (16 + stored_len_i)`.
- The "index region" runs from `index_offset` up to the signature section, or up to the trailer if there is no signature. The block table frame starts at `index_offset`. The catalog frame MUST start exactly where the block table frame ends and MUST end exactly at the end of the index region. Anything else is an error.
- Because the index comes last, an archive can be written as a stream (for example to a pipe). Reading assumes a seekable input.

## 3. Header (32 fixed bytes + E extension bytes)

| Offset | Size | Name | Value |
|---|---|---|---|
| 0 | 8 | magic | `89 45 5A 50 5A 0D 0A 1A` (`\x89EZPZ\r\n\x1a`) |
| 8 | 1 | version_major | `1` |
| 9 | 1 | version_minor | `0` |
| 10 | 2 | flags | bit 0: encrypted, bit 1: signed. Other bits 0 |
| 12 | 4 | ext_len | header extension length E (≤ 65536) |
| 16 | 16 | archive_id | 16 random bytes, new for each archive |
| 32 | E | ext | header extension records (§4) |

- The magic starts with `0x89` (outside ASCII) and contains `\r\n` and `\x1a` for the same reason PNG does: if a text-mode transfer rewrites line endings or strips the eighth bit, the magic breaks and the damage shows up at once.
- Decoders MUST reject a version_major other than 1. version_minor is reserved for backward-compatible additions, and decoders SHOULD read files with an unknown minor version.
- Decoders MUST reject files that have unknown flag bits set.
- archive_id is part of the encryption nonce, so in encrypted archives it MUST come from a cryptographically secure random source.

## 4. Header extension records

The `ext` area is a sequence of records:

| Size | Name |
|---|---|
| 1 | type |
| 2 | len (u16) |
| len | value |

- Bit 7 of type (`0x80`) marks a critical record. Decoders MUST reject unknown critical records and MUST skip unknown non-critical ones.
- Decoders MUST reject a file in which the same record type appears twice.

### 4.1 `0x81` encryption record (critical)

| Offset | Size | Name | Value |
|---|---|---|---|
| 0 | 1 | cipher | `1` = XChaCha20-Poly1305 |
| 1 | 1 | kdf | `1` = Argon2id (v1.3) |
| 2 | 16 | salt | random |
| 18 | 4 | m_kib | memory (KiB) |
| 22 | 4 | t_cost | iterations |
| 26 | 4 | p_lanes | parallelism |

- len ≥ 30. Extra trailing bytes are ignored.
- Decoders MUST reject unknown cipher or kdf values.
- The encryption bit in the header flags MUST match the presence of this record.

## 5. Frames

Data blocks, the block table, and the catalog all start with the same 16-byte frame header.

| Offset | Size | Name | Value |
|---|---|---|---|
| 0 | 4 | magic | `EZBK` data block / `EZBT` block table / `EZCT` catalog |
| 4 | 1 | codec | §6 |
| 5 | 1 | flags | bit 0: encrypted. Other bits 0 |
| 6 | 2 | reserved | 0 (MUST) |
| 8 | 4 | raw_len | size after decoding |
| 12 | 4 | stored_len | number of stored bytes that follow |
| 16 | stored_len | payload | compressed data (when encrypted: ciphertext followed by a 16-byte tag) |

- A data block's raw_len MUST be at least 1 and at most 256 MiB (268,435,456).
- The raw_len of the block table and of the catalog MUST NOT exceed 1 GiB.
- The block table frame MUST NOT be encrypted. The encryption bit of data blocks and of the catalog MUST equal the encryption bit in the header.
- Index frames (block table and catalog) MUST use codec 0, 1, or 2. The reference implementation uses zstd level 19.

## 6. Codecs

| id | Name | Payload |
|---|---|---|
| 0 | store | The original bytes. Payload length (after decryption) == raw_len |
| 1 | zstd | One or more RFC 8878 Zstandard frames |
| 2 | lzma2 | A dictionary-size property byte followed by a raw LZMA2 stream |
| 3 | brain | A table-size byte followed by an arithmetic-coded bit stream (§6.4) |
| 4 | brain-fast | Same layout as brain, coded with fewer models (§6.5) |
| 5-255 | reserved | Unknown codecs MUST be treated as an error |

Encoders SHOULD store a block with codec 0 when compression does not make it smaller.

### 6.1 store
The payload (after decryption, in an encrypted archive) is the original data, and its length MUST equal raw_len. The frame's stored_len is therefore raw_len in a plain archive and raw_len + 16 in an encrypted one.

### 6.2 zstd
- One or more concatenated RFC 8878 frames. Skippable frames are allowed.
- The window size MUST NOT exceed 2^28 bytes, and decoders MUST support windows up to 2^28 bytes.
- The total decoded length MUST equal raw_len.

### 6.3 lzma2
- The first byte `p` (0 ≤ p ≤ 32) is the LZMA2 dictionary-size property from §5.3.1 of the xz file format specification. Dictionary size = `(2 | (p & 1)) << (p / 2 + 11)` bytes (256 MiB at p = 32).
- The rest is a sequence of LZMA2 chunks as defined by the xz specification, and it MUST end with the end marker (`0x00`). Bytes left after the end marker are an error.
- Decoders MUST reject p > 32. This guards against decompression bombs and memory exhaustion.

### 6.4 brain (brain codec): normative algorithm

The brain codec predicts one bit at a time and codes it with an arithmetic coder. Compressor and decompressor MUST perform exactly the computations in this section. Only integer arithmetic is used, so every CPU produces the same result, and changing a single constant breaks compatibility.

#### 6.4.1 Payload layout
- Byte 0: `tb`, the hash table size exponent (each table has 2^tb u16 slots). Decoders MUST accept only 16 ≤ tb ≤ 25. The reference encoder uses `tb = clamp(ceil(log2(raw_len)) + 1, 16, 24)`.
- From byte 1 on: the arithmetic coder output (§6.4.10).
- Each byte of the original data is coded as 8 bits, most significant bit first.
- Memory use (informative): about `16 × 2^tb + 2^(tb-1) + raw_len + 5 MiB` bytes, roughly 330 MiB for tb = 24 with a 64 MiB block.

#### 6.4.2 Basic functions

**squash** is the logistic function. Its input d is a log-odds value scaled by 256, and its output is a 12-bit probability (0 to 4095).

```
T = [1,2,3,6,10,16,27,45,73,120,194,310,488,747,1101,1546,
     2047,2549,2994,3348,3607,3785,3901,3975,4022,4050,4068,4079,
     4085,4089,4092,4093,4094]
squash(d):
  if d >  2047: return 4095
  if d < -2047: return 1
  w = d & 127                 # two's-complement bitwise AND
  i = (d >> 7) + 16           # arithmetic shift
  return (T[i]*(128-w) + T[i+1]*w + 64) >> 7
```

**stretch** is the inverse of squash, kept as a 4096-entry i16 table.

```
pi = 0
for x in -2047 ..= 2047:
  v = squash(x)
  for i in pi ..= v: S[i] = x
  pi = max(pi, v + 1)
for i in pi .. 4096: S[i] = 2047
stretch(p) = S[p]
```

**hash2** uses 32-bit arithmetic (everything modulo 2^32).

```
hash2(a, b):
  h = a*0x9E3779B1 + b*0x85EBCA6B
  h = h ^ (h >> 16)
  h = h * 0x2C1B3C6D
  h = h ^ (h >> 13)
  return h
```

#### 6.4.3 Adaptive probability counters and bit histories

An **adaptive probability counter** (u32) holds p22, the 22-bit probability that the next bit is 1, and n, a 10-bit observation count. With the encoding below, all-zero memory is the initial state (p = ½, n = 0).

```
cnt = ((p22 XOR 0x200000) << 10) | n
p12(cnt) = ((cnt >> 10) XOR 0x200000) >> 10

RECIP[n] = floor(131072 / (2n + 3))      # = 65536/(n+1.5), n = 0..1023

update(cnt, y, limit):
  n = cnt & 1023
  p = (cnt >> 10) XOR 0x200000           # computed as i64
  target = y ? (2^22 - 1) : 0
  p = p + (((target - p) * RECIP[n]) >> 16)
  if n < limit: n = n + 1
  cnt = ((p XOR 0x200000) << 10) | n
```

Limits: `60` for the order-0 counters, `1023` for the state maps and the match model.

A **bit history (state)** (u16, low 13 bits used) records, for one situation (a context node), the number of 0s seen so far, n0 (6 bits), the number of 1s, n1 (6 bits), and the last bit (1 bit). When one count grows while the other is large, the other is cut down, so recent behavior counts for more than old behavior.

```
state = n0 | (n1 << 6) | (last << 12)          # starts at 0 (no history)
next_state(s, y):
  n0 = s & 63 ; n1 = (s >> 6) & 63
  if y == 1: n1 = min(n1 + 1, 63) ; if n0 > 2: n0 = (n0 >> 1) + 1
  else:      n0 = min(n0 + 1, 63) ; if n1 > 2: n1 = (n1 >> 1) + 1
  return n0 | (n1 << 6) | (y << 12)
total(s) = (s & 63) + ((s >> 6) & 63)
```

A **state map** holds 8192 counters per model. Across all situations, it learns how often a 1 followed each history. A situation seen for the first time (state 0) therefore starts from a probability learned from earlier experience.

#### 6.4.4 State variables

| Name | Initial value | Meaning |
|---|---|---|
| c0 | 1 | bits of the current byte seen so far, with a leading 1 (1..255) |
| nib | 1 | bits of the current nibble (4 bits) seen so far, with a leading 1 (1..15) |
| bitpos | 0 | position of the current bit within the byte (0..7) |
| c4 | 0 | the last 4 bytes (most recent in the low byte) |
| c8 | 0 | the 4 bytes before those |
| word, pword | 0 | hash of the current word / of the previous word |
| hist | empty | every byte so far |
| m_ptr, m_len, lq | 0 | match model state (§6.4.6) |

`c1 = c4 & 0xFF` (the previous byte). During learning (§6.4.8 step 6) c0 briefly becomes 256..511 and returns to 1 at the byte boundary.

#### 6.4.5 Nine context models and order 0

At every byte boundary, compute these nine context values:

| i | Name | ctx_i |
|---|---|---|
| 0 | order 1 | `c1` |
| 1 | order 2 | `c4 & 0xFFFF` |
| 2 | order 3 | `c4 & 0xFFFFFF` |
| 3 | order 4 | `c4` |
| 4 | order 6 | `hash2(c4, c8 & 0xFFFF)` |
| 5 | word | `word == 0 ? hash2(c1, 0x5757) : word` |
| 6 | word pair | `hash2(word, pword + 0x3131)` |
| 7 | sparse 1 | `c4 & 0x00FFFF00` |
| 8 | sparse 2 | `c4 & 0xFF0000FF` |

**Hash tables**: one per model, each an array of 64-byte lines. A line has 32 u16 slots, split into 2 ways of 16 slots. Slot 0 of a way is a check value, and slots 1 to 15 hold the bit histories of the 15 nodes of a nibble's binary tree. Model 0 uses `bits = min(tb, 18)` and the others use `bits = tb` (2^bits u16 slots). There are `2^(bits-5)` lines, and `shift = 32 - (bits - 5)`. Tables start out zeroed.

```
find(table, h):                               # returns (line, way start slot: 0 or 16)
  L   = h >> shift
  chk = ((h * 0x2545F491) >> 16) | 1          # multiply mod 2^32, keep the upper 16 bits
  if line[L][0]  == chk: return (L, 0)
  if line[L][16] == chk: return (L, 16)
  w = (total(line[L][1]) < total(line[L][17])) ? 0 : 16   # replace the less used way (16 on a tie)
  line[L][w .. w+16] = 0 ; line[L][w] = chk
  return (L, w)
```

**Selecting buckets**: at the start of each byte (c0 = 1) and right after its first 4 bits (c0 = 16..31), for every i,
`base_i = find(table_i, hash2(ctx_i + (i << 28), c0))` (the addition wraps modulo 2^32). The selection after 4 bits uses the same ctx_i values computed at the byte boundary. The current node of model i, `node_i`, is the u16 slot at the way start of `base_i` plus nib.

**Order 0**: a 256-entry counter array `t0`, indexed as `t0[c0]`.

#### 6.4.6 Match model

The match model finds long repeats, such as the same sentence or the same block of code appearing again.

- Hash table `mt`: `2^(tb-3)` u32 entries, initially 0. `mshift = 32 - (tb - 3)`.
- State: `m_ptr` (the position in hist used for the prediction), `m_len` (current match length, initially 0), and counters `m_sm[64]`.
- `MATCH_MIN = 6`, `MATCH_KEEP = 16`.

At every byte boundary (after the new byte has been appended to hist, with `pos = len(hist)`):

```
last = hist[pos-1]
if m_len > 0:
  if hist[m_ptr] == last:   m_len = min(m_len + 1, 65535)
  elif m_len >= MATCH_KEEP: m_len = 1          # a long match missed by one byte: keep following it with low confidence
  else:                     m_len = 0
  if m_len > 0: m_ptr += 1
if pos >= 6:
  a = u32le(hist[pos-4], hist[pos-3], hist[pos-2], hist[pos-1])
  b = u32le(hist[pos-6], hist[pos-5], 0, 0)
  s = hash2(a, b + 0x6D6D6D6D) >> mshift
  if m_len < MATCH_KEEP and mt[s] > 0:
    cand = mt[s] ; l = 0
    while l < 32 and l < cand and hist[cand-1-l] == hist[pos-1-l]: l += 1
    if l >= MATCH_MIN and l > m_len: m_len = l ; m_ptr = cand
  mt[s] = pos
lq = (m_len < 16) ? m_len : min(16 + ((m_len - 16) >> 2), 31)
```

#### 6.4.7 Prediction (every bit)

Build the input vector x[12]:

```
s_i   = node_i & 0x1FFF                         # bit history of model i's current node
x[i]  = stretch(p12(SM_i[s_i]))                 i = 0..8  (SM_i = state map of model i)
x[9]  = stretch(p12(t0[c0]))
x[10] = 0 ; mctx = none
if m_len > 0:
  pb = hist[m_ptr] | 0x100 ; sh = 8 - bitpos
  if (pb >> sh) == c0:                 # the bits so far agree with the predicted byte
    e = (pb >> (sh - 1)) & 1           # predicted bit
    mctx = lq*2 + e
    x[10] = stretch(p12(m_sm[mctx]))
x[11] = 256                            # bias
```

**Three neural mixers**: each has 256 weight sets of 12 i32 weights, all initialized to `16384`.

```
mix(M, sel):
  dot = Σ_{i=0..11} x[i] * M.w[sel][i]          # multiply and sum in i64
  st  = clamp(dot >> 16, -2047, 2047)
  M.pr = squash(st) ; M.sel = sel
  return st

selA = c0                                       # which bit of the byte this is
selB = (mctx is none ? 0 : lq) * 8 + bitpos     # how long the match has been running
selC = c1                                       # what the previous byte was
st   = ((mix(A,selA) + mix(B,selB) + mix(C,selC)) * 21846) >> 16    # average of the three
pm   = squash(st)
```

The average st lies in -2048 ..= 2047 (-2048 can occur). squash accepts it as is, and the APM's `s = st + 2048` then falls in 0..4095. A table-based squash must cover -2048 as well.

**Two APM stages** (tables that refine the prediction once more):

```
APM(n): u16 table with n×33 entries, t[i*33+j] = squash((j-16)*128) * 16
pp(A, st, cx):
  s = st + 2048 ; lo = s >> 7 ; w = s & 127
  A.idx = cx*33 + lo + (w >> 6)                 # the nearer entry, updated later
  return (t[cx*33+lo]*(128-w) + t[cx*33+lo+1]*w) >> 11

a1 = pp(APM1(256),   st, c0)
a2 = pp(APM2(65536), st, c0 | (c1 << 8))
p  = clamp((pm + a1 + 2*a2 + 2) >> 2, 1, 4095)  # probability that the next bit is 1 (/4096)
```

#### 6.4.8 Learning (after coding or decoding bit y)

In this order:

```
1. For every i: update(SM_i[s_i], y, 1023) ; write next_state(node_i, y) back into the table slot of node_i
2. update(t0[c0], y, 60)
3. If mctx is set: update(m_sm[mctx], y, 1023)
4. For each of the mixers A, B, C:
     err = ((y << 12) - M.pr) * 16
     M.w[M.sel][i] = wrap32(M.w[M.sel][i] + ((x[i] * err) >> 16))   i = 0..11
5. For APM1 and APM2 (compute in i32, since g can exceed the u16 range):
     g = (y << 16) + (y << 7) - y - y
     t[idx] = t[idx] + ((g - t[idx]) >> 7)       # the result always stays in 0..65535
6. c0 = c0*2 + y ; nib = nib*2 + y ; bitpos += 1
7. If bitpos == 8: byte boundary processing (§6.4.9)
   If bitpos == 4: nib = 1, then select buckets (§6.4.5)
```

#### 6.4.9 Byte boundary processing

```
c = c0 & 0xFF
hist.push(c)
c8 = (c8 << 8) | (c4 >> 24)
c4 = (c4 << 8) | c
lc = ASCII lowercase of c                  # only 'A'..'Z' change
if 'a' <= lc <= 'z' or c >= 0x80:          # 0x80 and above: UTF-8 letters (Hangul, kana, ...)
  word = hash2(word + lc, 0x77770001)
elif word != 0:
  pword = word ; word = 0
update the match model (§6.4.6)
c0 = 1 ; nib = 1 ; bitpos = 0
recompute the 9 ctx values, then select buckets (§6.4.5)
```

At the very start, before any byte, the ctx values are also computed and the buckets selected once.

#### 6.4.10 Arithmetic coder (32-bit)

```
x1 = 0, x2 = 0xFFFFFFFF                      # u32
xmid(p) = x1 + ((x2 - x1) >> 12) * p + ((((x2 - x1) & 0xFFF) * p) >> 12)     # u32, cannot overflow

encode(y, p):
  m = xmid(p)
  if y: x2 = m  else: x1 = m + 1
  while ((x1 XOR x2) & 0xFF000000) == 0:
    output(x2 >> 24) ; x1 = x1 << 8 ; x2 = (x2 << 8) | 255
at the end: output x1 as 4 big-endian bytes

decode: x = the first 4 bytes (big-endian); reading past the end of the input yields 0
  m = xmid(p) ; y = (x <= m) ? 1 : 0
  if y: x2 = m, otherwise: x1 = m + 1
  while ((x1 XOR x2) & 0xFF000000) == 0:
    x1 = x1 << 8 ; x2 = (x2 << 8) | 255 ; x = (x << 8) | next_byte
```

Decoding stops after exactly raw_len bytes. Any remaining input is ignored (the block hash guarantees integrity).

### 6.5 brain-fast (codec 4)

brain-fast is the brain codec with fewer models. Depending on the processor it runs 1.2 to 2 times as fast as codec 3, and its output is 1.5 to 8% larger: the least on plain text, the most on program files. Everything in §6.4 applies, with the changes below.

- **Payload layout** (§6.4.1): unchanged, including the rule 16 ≤ tb ≤ 25. Memory use (informative): about `10 × 2^tb + 2^(tb-1) + raw_len + 1 MiB` bytes, roughly 185 MiB for tb = 24 with a 16 MiB block.
- **Context models** (§6.4.5): only models 0 to 5 (order 1, order 2, order 3, order 4, order 6, word). Models 6, 7, and 8 do not exist, so there are six hash tables and six state maps. Table sizes follow §6.4.5: model 0 uses `bits = min(tb, 18)` and models 1 to 5 use `bits = tb`. At every byte boundary only ctx_0 to ctx_5 are computed, and bucket selection runs over i = 0..5 with the same formula.
- **Input vector** (§6.4.7): x has 9 entries.

  ```
  x[i] = stretch(p12(SM_i[s_i]))       i = 0..5
  x[6] = stretch(p12(t0[c0]))
  x[7] = the match model input, computed exactly like x[10] in §6.4.7 (0 when mctx is none)
  x[8] = 256                           # bias
  ```

  mctx is set exactly as in §6.4.7, and learning step 3 still updates `m_sm[mctx]`.

- **Mixers**: two mixers, A and C, each with 256 weight sets of 9 i32 weights, all initialized to `16384`. Mixer B does not exist. `mix` is the function from §6.4.7 with the sum taken over i = 0..8.

  ```
  st = (mix(A, c0) + mix(C, c1)) >> 1   # arithmetic shift; st stays in -2047 ..= 2047
  pm = squash(st)
  ```

- **APM**: only APM1. APM2 does not exist.

  ```
  a1 = pp(APM1(256), st, c0)
  p  = clamp((pm + 3*a1 + 2) >> 2, 1, 4095)
  ```

- **Learning** (§6.4.8): step 1 runs for i = 0..5, step 4 updates mixers A and C over i = 0..8, and step 5 updates APM1 only. Steps 2, 3, 6, and 7 are unchanged.

The counters and bit histories (§6.4.3), the match model (§6.4.6), byte boundary processing (§6.4.9), and the arithmetic coder (§6.4.10) are the same as in codec 3.

## 7. Block table (`EZBT`, always plaintext)

The decoded frame payload is laid out as follows. Values are grouped by column so that they compress well.

```
varint  version = 1
varint  n                       # number of data blocks
varint  raw_len[n]              # 1 ..= 268435456
varint  stored_len[n]
bytes32 hash[n]                 # BLAKE3( the block's 16-byte frame header ‖ payload )
```

- Block offsets follow from the formula in §2, and the end of the last block MUST equal index_offset exactly.
- Bytes left over after parsing are an error (MUST).
- The raw_len and stored_len in each data block's frame header MUST equal the values in the block table.
- Because the block table is never encrypted, the integrity of every block can be checked without the password. This reveals only the block sizes, which the file length exposes anyway.

## 8. Catalog (`EZCT`, may be encrypted)

```
varint   version = 1
u8       hash_len               # file hash length: 0, 16, 32
svarint  created                # creation time (Unix seconds)
varint   creator_len ; bytes creator   # creating program (UTF-8)

# chunk layout: chunk ids are numbered from 0 in this order
varint   block_count            # must equal n in the block table
varint   chunk_count[block_count]
varint   chunk_len[Σ chunk_count]       # block by block, each ≥ 1
#   sum of chunk_len for block b == raw_len[b]  (MUST)

# entries: sorted by path (bytewise), no duplicates
varint   entry_count = m
m times: varint shared ; varint suffix_len ; bytes suffix   # the prefix shared with the previous path is omitted
u8       type[m]                # 0 file, 1 directory, 2 symbolic link
varint   mode[m]                # Unix permission bits (& 0o7777)
svarint  mtime_delta[m]         # modification time (seconds), difference from the previous entry (first entry: from 0)
varint   mtime_nsec[m]          # 0 ..= 999999999

# file entries only, in entry order (f = number of files)
u8       transform[f]           # §9
varint   nchunks[f]
svarint  chunk_ref_delta[Σ nchunks]   # ref - (previous ref + 1). "previous ref" carries over
                                      # across files (it is not reset per file); only the very first is -1
bytes    file_hash[f × hash_len]      # first hash_len bytes of BLAKE3(original file content)

# symbolic link entries only, in entry order
varint   target_len ; bytes target    # UTF-8
```

- File content is the file's chunks joined in order, with the transform undone.
- File sizes are not stored separately. The sum of the chunk lengths is the size, since transforms do not change length.
- Several files can point to the same chunk; that is how deduplication works. Chunks that no file references are allowed.
- A hash_len other than 0, 16, or 32 is an error (MUST).
- Paths are relative to the archive root. Entries for parent directories SHOULD be present but are optional, and extractors MUST create missing parent directories.
- No entry may lie "inside" a file or symbolic link entry (MUST). For example, a link `a` → `/etc` together with a file `a/x` must be rejected.
- mode: decoders ignore (mask off) bits outside 0o7777. A symbolic link's mode is informational. Extractors SHOULD NOT restore setuid, setgid, or sticky bits unless the user explicitly asks for it. Directory permissions and times SHOULD be applied after every entry, files and symbolic links included, has been created, because creating a symbolic link also changes its directory's time.
- Bits of mode outside 0o7777 are reserved and ignored, as an exception to the rule of rejecting what a decoder does not know (§17).
- mtime is the entry's own time (for a symbolic link, the time of the link itself).
- Path rules (MUST):
  - UTF-8, 1 to 4096 bytes, with `/` as the separator
  - no leading or trailing `/`, and no empty, `.`, or `..` components
  - no NUL (`\0`) or backslash (`\`), and no colon (`:`) in the first component, which blocks drive forms such as `C:` and `C:foo`
- Decoders MUST treat sort-order violations, duplicate paths, out-of-range chunk references, and leftover bytes as errors.

## 9. File transforms (executable preprocessing)

These transforms turn relative jump targets in machine code into absolute addresses. Calls to the same function then become identical byte sequences, which compress better. Lengths do not change, and the position `i` is counted from the start of the file.

| id | Name |
|---|---|
| 0 | none |
| 1 | x86 (E8/E9) |
| 2 | ARM64 (BL) |

```
x86(buf, encode):
  i = 0
  while i + 5 <= len(buf):
    if buf[i] == 0xE8 or buf[i] == 0xE9:
      v = u32le(buf[i+1..i+5])
      if (v >> 23) == 0 or (v >> 23) == 0x1FF:          # only when |displacement| < 2^23
        r = (encode ? v + i : v - i) & 0x00FFFFFF          # mod 2^32
        if r & 0x00800000: r |= 0xFF000000                 # sign-extend from 24 bits
        buf[i+1..i+5] = u32le(r)
      i += 5
    else:
      i += 1

arm64(buf, encode):
  for i in 0, 4, 8, ... while i + 4 <= len(buf):
    w = u32le(buf[i..i+4])
    if (w & 0xFC000000) == 0x94000000:
      imm = w & 0x03FFFFFF ; pc = i >> 2
      imm = (encode ? imm + pc : imm - pc) & 0x03FFFFFF
      buf[i..i+4] = u32le(0x94000000 | imm)
```

Both transforms are exactly reversible. For x86, a transformed value stays within the same range, so the decoder can tell which values were changed.

## 10. Signature section (104 bytes, optional)

| Offset | Size | Name | Value |
|---|---|---|---|
| 0 | 4 | magic | `EZSG` |
| 4 | 1 | algo | `1` = Ed25519 |
| 5 | 3 | reserved | 0 |
| 8 | 32 | public_key | |
| 40 | 64 | signature | |

- Signed message = `"EZPZ-v1 signature\0"` (18 bytes) ‖ `root_hash` (§12).
- Decoders MUST perform "strict" Ed25519 verification, meaning RFC 8032 verification with these restrictions: S < L (canonical), the public key A and R canonically encoded and not small-order points, and the cofactorless equation `[S]B = R + [k]A`. If a signature is present and does not verify, decoders MUST reject the archive.
- A verifier that expects a signature (for example because a specific public key was given) MUST also reject archives that have no signature. Otherwise someone could strip the signature, recompute the hashes, and pass the result off as genuine.
- A signature proves only that the holder of this public key signed this root_hash, which covers the entire content of the archive. It does not prove who created the archive: anyone can add their own signature to an existing archive, and an encrypted archive can be re-signed without the password. Whether to trust the key is up to the user.
- When the verifier specifies an expected public key, the public_key in the signature section MUST equal it exactly. No other hash protects the signature section, so without this comparison a file re-signed with a different key also looks "valid".

## 11. Trailer (last 64 bytes)

| Offset | Size | Name | Value |
|---|---|---|---|
| 0 | 8 | index_offset | start of the block table frame |
| 8 | 8 | archive_len | total file length |
| 16 | 32 | root_hash | BLAKE3(entire header ‖ entire index region), see §12 |
| 48 | 4 | flags | bit 0: signature section present |
| 52 | 4 | check | first 4 bytes of BLAKE3(trailer bytes 0..52) |
| 56 | 8 | end_magic | `EZPZEND\x1a` |

- An archive_len that differs from the actual file length is an error (MUST). This catches truncated files and trailing garbage.
- The signature bit in the trailer flags MUST equal the signature bit in the header flags. Decoders MUST reject unknown trailer flag bits.
- `header_length ≤ index_offset` and `index_offset + 32 ≤ end of the index region` MUST hold.

## 12. Integrity chain and archive fingerprint

```
root_hash = BLAKE3( entire header (32+E bytes) ‖ entire index region )

signature ──▶ root_hash ─┬─ header (flags, archive_id, encryption parameters)
 (optional)   (trailer)   └─ index region
                              ├─ block table: hash[k] ──▶ data block k (frame header + stored bytes)
                              └─ catalog: file_hash ──▶ restored original file content
```

- The trailer is protected by its check field and by structural checks (archive_len, index_offset, matching flags), and the signature section by signature verification. Accidental damage to any byte (transfer errors, disk errors, truncation) is therefore caught somewhere along this chain.
- The hashes use no secret key, so they do not stop deliberate changes: an attacker can modify the content and recompute every hash. To detect deliberate tampering, either (a) require a signature from a trusted public key (§10), or (b) compare against a root_hash kept somewhere trustworthy. In an encrypted archive only someone who knows the password can change the content (AEAD), but telling apart changes made by different people who share the password again takes a signature.
- `root_hash` also serves as the archive's fingerprint (digest). Recording just these 32 bytes, for example on a public board or a blockchain, makes it possible to confirm later that an archive is the same one.
- Decoders MUST check root_hash before parsing the index, MUST check each block's hash before writing data from it, and MUST check each file's hash after extracting it when hash_len > 0.

## 13. Encryption

```
password = the user's password, Unicode NFC-normalized, as UTF-8 bytes
master = Argon2id(password, salt, m_kib, t_cost, p_lanes, 32-byte output, no secret or associated data, version 0x13)
key    = BLAKE3.derive_key("ezpz v1 data encryption key", master)
nonce  = archive_id(16) ‖ u64le(counter)          # 24 bytes
ciphertext = XChaCha20-Poly1305(key, nonce, aad = the 16-byte frame header, plaintext = compressed data)
```

| Target | counter |
|---|---|
| data block k | k |
| catalog | 2^64 - 1 |

- Data is compressed first, then encrypted. A frame's stored_len is the compressed size plus 16 (the tag), and the whole frame header is bound as aad, so the codec and the sizes cannot be altered either.
- Reordering blocks breaks decryption, because the counters no longer match.
- Since the catalog is encrypted, file names, sizes, times, and the folder structure are all hidden (zip encryption leaves names visible).
- Decoders MUST reject KDF parameters outside `1 ≤ p ≤ 16`, `1 ≤ t ≤ 64`, `8p ≤ m_kib ≤ 4 GiB (4194304)`, so that a malicious file cannot exhaust memory.
- Reference encoder defaults: m = 64 MiB, t = 3, p = 1.
- The reason for NFC: the same Korean (Hangul) password can arrive with its syllables composed (NFC) or split into jamo (NFD), depending on the operating system or the input method. Without normalization, a correct password could fail to open the archive.
- A wrong password fails when the catalog is decrypted (tag verification).

## 14. Reading procedure (decoder)

1. Check that the file length L ≥ 96.
2. Read the 32-byte header and check the magic, version, and flags; then read the E extension bytes and parse the records.
3. Read the last 64 bytes (the trailer) and check end_magic, check, flags, and archive_len == L.
4. Check that the signature flags agree. Index region = [index_offset, L - 64 - (signed ? 104 : 0)).
5. Read the index region and check BLAKE3(header ‖ index region) == root_hash.
6. If the archive is signed, verify the signature over root_hash.
7. Decode and parse the block table frame, compute the block offsets, and check that they end at index_offset.
8. Catalog frame: check the magic `EZCT`, that it starts right after the block table and ends at the end of the index region, that its encryption bit equals the header's, that codec ∈ {0,1,2}, and that raw_len ≤ 1 GiB. If encrypted, derive the key and decrypt (aad = the catalog frame header). Decode and parse it, checking every constraint in §8.
9. To read a file: read only the blocks it needs → check each block hash → decrypt → decompress (check raw_len) → cut out and join the chunks → undo the transform → check the file hash.

Without the password, a reader can still carry out steps 1 to 7 and check every block hash (a damage check), and verify the signature (which also detects tampering when a signature is present and matches a trusted key).

## 15. Encoder recommendations (informative)

Decoders do not need anything in this section. It describes how the reference encoder makes good use of the format.

### 15.1 Classifying and ordering files
- Classify each file from its first 64 KiB: executables (architecture detected from ELF, Mach-O, or PE headers), already-compressed files (detected by extension, magic bytes, and a quick trial compression), and everything else.
- Group already-compressed files into separate blocks stored with codec 0, which saves time.
- Order executables by (architecture, size), so that similar builds of the same program sit together and one huge file does not push related files far apart.
- Order everything else by (extension, file name, path), so that, for example, files with the same name from different versions end up side by side.

### 15.2 Content-defined chunking
- FastCDC (2020) with a minimum of 16 KiB, an average of 64 KiB, and a maximum of 256 KiB.
- Duplicates are detected by each chunk's BLAKE3 hash. Chunks never cross block boundaries.

### 15.3 Levels (reference implementation)

| Level | Codec | Block |
|---|---|---|
| 1 | zstd 1 | 4 MiB |
| 2 | zstd 3 | 4 MiB |
| 3 | zstd 6 | 8 MiB |
| 4 | zstd 9 (+LDM) | 8 MiB |
| 5 | zstd 12 | 16 MiB |
| 6 | zstd 16 | 16 MiB |
| **7 (default)** | zstd 19 (+LDM) | 32 MiB |
| 8 | zstd 22 | 64 MiB |
| 9 | tries zstd 22 and LZMA2 9e, keeps the smaller | 64 MiB |
| 10 (`--max`) | brain-fast | 16 MiB |
| 11 | brain | 64 MiB |

Blocks are independent of each other, so they are compressed and extracted on several cores at once. A brain codec block can only be coded on one core, which is why level 10 uses 16 MiB blocks: even a 50 MB input keeps several cores busy. Going from 64 MiB to 16 MiB blocks makes brain codec output about 2% larger.

## 16. Security considerations

- **Path attacks (zip-slip)**: always check the path rules in §8. Extractors SHOULD create symbolic links after writing all regular files, and SHOULD NOT create links that point outside the extraction folder or to absolute paths unless the user allows it.
- **Decompression bombs**: the limits on block raw_len (256 MiB), index size (1 GiB), LZMA2 dictionary size, brain codec tb (codecs 3 and 4), and zstd window cap the memory a single block can use. Extractors can compute the total extracted size in advance (the sum of the chunk lengths) and show it to the user or enforce a limit.
- **Tampering**: check block hashes before writing and file hashes afterwards. If a signed archive's signature does not verify, extract nothing. See §12 for protection against deliberate tampering.
- **Name collisions**: on case-insensitive file systems (the default on Windows and macOS), `A.txt` and `a.txt` are the same file, and macOS may treat names that differ only in Unicode normalization as the same name. On Windows, reserved names such as `CON`, `NUL`, and `COM1`, and a `:` inside a name (alternate data streams), are also dangerous. An extractor that detects such a collision SHOULD rename the file or stop, and not overwrite.
- **Permissions**: setuid, setgid, and sticky bits are not restored by default (§8).
- **Limits of encryption**: the number and sizes of blocks are not hidden, and the encryption is only as strong as the password.

## 17. Versioning and extensions

- Incompatible changes increase version_major.
- New features are added as (a) new codec ids, (b) header extension records, with the critical bit if needed, or (c) new flag bits. Older decoders are designed to reject anything they do not understand, with a clear error, instead of silently producing wrong output.

## 18. Future work (candidates for v1.x and later)

- **Built-in knowledge (pretraining)**: start the brain codec from a state that has already learned common text and code patterns, which helps small files most.
- **Recovery records**: Reed-Solomon parity blocks that repair partial damage.
- **Append**: add files to an existing archive while deduplicating against the chunks already stored.
- **Deflate recompression**: decompress the deflate streams inside zip, docx, and png files to compress them better, and restore the original bytes exactly on extraction.
- **Browser decoder**: open archives directly on the web with WebAssembly.
- **Streaming read**: a mode that can be decoded in order without seeking.

## Appendix A. Independent implementation check

To find out whether this specification is precise enough to build a compatible implementation from the document alone, a separate Python decoder was written from this document only, without looking at the Rust reference implementation (`tools/ezpz_reader.py`).

| Check | Result |
|---|---|
| Parsing the header, trailer, root_hash, block table, and catalog | pass |
| Decoding store / zstd / LZMA2 / brain codec (brain codec at tb=16 and tb=20) | pass, byte-for-byte identical to the reference implementation |
| Decoding brain-fast (codec 4, added on 2026-10-02 from §6.5 alone, at tb=16 and tb=20) | pass, byte-for-byte identical to the reference implementation |
| Undoing the x86 and ARM64 file transforms | pass |
| Deduplicated chunk references (including references across blocks) | pass |
| Encryption (Argon2id + BLAKE3 derive_key + XChaCha20-Poly1305), rejecting a wrong password | pass |
| Ed25519 signature verification, rejecting a mismatched public key or an unsigned file | pass |
| One-byte tampering tests (block, catalog, header, trailer, signature) | all rejected at the expected step |

About 20 unclear points found during this check (a miscalculated LZMA2 dictionary limit, the base of the chunk reference delta, the length of encrypted store blocks, the signature key comparison, password normalization, and others) have all been fixed in this edition.
