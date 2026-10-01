//! Reversible, length-preserving file transforms for machine code (SPEC.md §9).
//! They turn relative branch targets into absolute ones so repeated calls to the
//! same function become identical byte patterns the compressor can find.

pub const XF_NONE: u8 = 0;
pub const XF_X86: u8 = 1;
pub const XF_ARM64: u8 = 2;
pub const XF_X86_64: u8 = 3;
pub const XF_X86_64_SPLIT: u8 = 4;
pub const XF_MAX: u8 = 4;

pub fn encode(xf: u8, buf: &mut [u8]) {
    match xf {
        XF_X86 => x86(buf, true),
        XF_ARM64 => arm64(buf, true),
        XF_X86_64 => x86_64(buf, true),
        XF_X86_64_SPLIT => x86_64_split_encode(buf),
        _ => {}
    }
}

pub fn decode(xf: u8, buf: &mut [u8]) {
    match xf {
        XF_X86 => x86(buf, false),
        XF_ARM64 => arm64(buf, false),
        XF_X86_64 => x86_64(buf, false),
        XF_X86_64_SPLIT => x86_64_split_decode(buf),
        _ => {}
    }
}

/// Converts a rel32 field at `off` (relative to instruction position `pos`) when the
/// displacement fits in 24 bits. The result stays in that range, so decoding can tell.
#[inline]
fn conv24(buf: &mut [u8], off: usize, pos: usize, enc: bool) {
    let v = u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
    let top9 = v >> 23;
    if top9 == 0 || top9 == 0x1FF {
        let pos = pos as u32;
        let mut r = if enc { v.wrapping_add(pos) } else { v.wrapping_sub(pos) } & 0x00FF_FFFF;
        if r & 0x0080_0000 != 0 {
            r |= 0xFF00_0000;
        }
        buf[off..off + 4].copy_from_slice(&r.to_le_bytes());
    }
}

/// One-byte opcodes whose ModRM byte can select RIP-relative addressing (SPEC.md §9).
const RIP_OP1: [bool; 256] = table(&[
    0x01, 0x03, 0x09, 0x0B, 0x21, 0x23, 0x29, 0x2B, 0x31, 0x33, 0x38, 0x39, 0x3A, 0x3B, 0x63, 0x80,
    0x81, 0x83, 0x84, 0x85, 0x88, 0x89, 0x8A, 0x8B, 0x8D, 0xC6, 0xC7, 0xF7, 0xFF,
]);
/// Second bytes of 0F xx opcodes handled the same way (SSE moves and arithmetic, MOVZX, MOVSX, ...).
const RIP_OP2: [bool; 256] = table(&[
    0x10, 0x11, 0x12, 0x13, 0x16, 0x17, 0x28, 0x29, 0x2A, 0x2E, 0x2F, 0x51, 0x54, 0x57, 0x58, 0x59,
    0x5A, 0x5C, 0x5E, 0x6F, 0x74, 0x76, 0x7F, 0xB6, 0xB7, 0xBE, 0xBF, 0xD6, 0xDB, 0xEB, 0xEF,
]);

const fn table(ops: &[u8]) -> [bool; 256] {
    let mut t = [false; 256];
    let mut i = 0;
    while i < ops.len() {
        t[ops[i] as usize] = true;
        i += 1;
    }
    t
}

/// x86-64: CALL/JMP rel32 as in `x86`, plus RIP-relative disp32 operands
/// (ModRM with mod = 00 and r/m = 101), which x86-64 code uses for most references
/// to global data and to the GOT. Positions that are examined are never modified,
/// so encoder and decoder walk the buffer identically.
fn x86_64(buf: &mut [u8], enc: bool) {
    let n = buf.len();
    let mut i = 0usize;
    while i + 5 <= n {
        let c = buf[i];
        if c == 0xE8 || c == 0xE9 {
            conv24(buf, i + 1, i, enc);
            i += 5;
            continue;
        }
        let mut j = i;
        if c == 0x66 || c == 0xF2 || c == 0xF3 {
            j += 1;
        }
        if j < n && buf[j] & 0xF0 == 0x40 {
            j += 1; // REX prefix
        }
        if j + 7 <= n && buf[j] == 0x0F && RIP_OP2[buf[j + 1] as usize] && buf[j + 2] & 0xC7 == 0x05 {
            conv24(buf, j + 3, i, enc);
            i = j + 7;
            continue;
        }
        if j + 6 <= n && RIP_OP1[buf[j] as usize] && buf[j + 1] & 0xC7 == 0x05 {
            conv24(buf, j + 2, i, enc);
            i = j + 6;
            continue;
        }
        i += 1;
    }
}

/// Transform 4: the smallest B in 23..=31 with 2^B >= n. Every address inside a file of
/// n bytes is then within the range that `conv_range` converts.
fn range_bits(n: usize) -> u32 {
    let mut b = 23;
    while b < 31 && (1u64 << b) < n as u64 {
        b += 1;
    }
    b
}

/// The rule of `conv24` with the range [-2^b, 2^b) instead of [-2^23, 2^23): a value in
/// the range has `pos` added (or subtracted) modulo 2^(b+1) and is sign-extended from bit
/// b, so it stays in the range; any other value is returned unchanged.
#[inline]
fn conv_range(v: u32, pos: u32, b: u32, enc: bool) -> u32 {
    let top = v >> b;
    if top == 0 || top == u32::MAX >> b {
        let m = ((2u64 << b) - 1) as u32;
        let mut r = if enc { v.wrapping_add(pos) } else { v.wrapping_sub(pos) } & m;
        if r >> b != 0 {
            r |= !m;
        }
        r
    } else {
        v
    }
}

/// Transform 4 pattern check at `i` with `end` as the end of the data: the offset of the
/// address field of a CALL (E8) or of a RIP-relative operand, or None. JMP (E9) is left alone.
#[inline]
fn split_site(b: &[u8], i: usize, end: usize) -> Option<usize> {
    let c = b[i];
    if c == 0xE8 {
        return Some(i + 1);
    }
    let mut j = i;
    if c == 0x66 || c == 0xF2 || c == 0xF3 {
        j += 1;
    }
    if j < end && b[j] & 0xF0 == 0x40 {
        j += 1; // REX prefix
    }
    if j + 7 <= end && b[j] == 0x0F && RIP_OP2[b[j + 1] as usize] && b[j + 2] & 0xC7 == 0x05 {
        return Some(j + 3);
    }
    if j + 6 <= end && RIP_OP1[b[j] as usize] && b[j + 1] & 0xC7 == 0x05 {
        return Some(j + 2);
    }
    None
}

/// Transform 4 (x86-64, split): every matched address field is converted with
/// `conv_range` and moved out of the code into a tail at the end of the file, last field
/// first, big-endian. The code that remains is identical wherever the same instructions
/// appear, even in another build where everything sits at different addresses.
fn x86_64_split_encode(buf: &mut [u8]) {
    let n = buf.len();
    let bits = range_bits(n);
    let mut out = Vec::with_capacity(n);
    let mut fields: Vec<u32> = Vec::new();
    let (mut i, mut run) = (0usize, 0usize);
    while i + 5 <= n {
        if let Some(f) = split_site(buf, i, n) {
            out.extend_from_slice(&buf[run..f]);
            let v = u32::from_le_bytes([buf[f], buf[f + 1], buf[f + 2], buf[f + 3]]);
            fields.push(conv_range(v, i as u32, bits, true));
            i = f + 4;
            run = i;
        } else {
            i += 1;
        }
    }
    out.extend_from_slice(&buf[run..]);
    for w in fields.iter().rev() {
        out.extend_from_slice(&w.to_be_bytes());
    }
    debug_assert_eq!(out.len(), n);
    buf.copy_from_slice(&out);
}

/// Inverse of `x86_64_split_encode`. The fields are taken from the end; `t`, the end of
/// the data still to be read, shrinks by 4 for each one, so every pattern check sees the
/// same bytes and the same distance to the end as it did in the encoder.
fn x86_64_split_decode(buf: &mut [u8]) {
    let n = buf.len();
    let bits = range_bits(n);
    let mut out = Vec::with_capacity(n);
    let (mut q, mut t, mut run) = (0usize, n, 0usize);
    while q + 5 <= t {
        if let Some(f) = split_site(buf, q, t) {
            out.extend_from_slice(&buf[run..f]);
            let pos = (out.len() - (f - q)) as u32;
            let w = u32::from_be_bytes([buf[t - 4], buf[t - 3], buf[t - 2], buf[t - 1]]);
            t -= 4;
            out.extend_from_slice(&conv_range(w, pos, bits, false).to_le_bytes());
            q = f;
            run = f;
        } else {
            q += 1;
        }
    }
    out.extend_from_slice(&buf[run..t]);
    debug_assert_eq!(out.len(), n);
    buf.copy_from_slice(&out);
}

/// x86 CALL (E8) / JMP (E9) rel32. Only displacements in [-2^23, 2^23) are
/// converted; the output stays in the same range, so the decoder can tell.
fn x86(buf: &mut [u8], enc: bool) {
    let n = buf.len();
    let mut i = 0usize;
    while i + 5 <= n {
        let op = buf[i];
        if op == 0xE8 || op == 0xE9 {
            let v = u32::from_le_bytes([buf[i + 1], buf[i + 2], buf[i + 3], buf[i + 4]]);
            let top9 = v >> 23;
            if top9 == 0 || top9 == 0x1FF {
                let pos = i as u32;
                let mut r = if enc {
                    v.wrapping_add(pos)
                } else {
                    v.wrapping_sub(pos)
                } & 0x00FF_FFFF;
                if r & 0x0080_0000 != 0 {
                    r |= 0xFF00_0000;
                }
                buf[i + 1..i + 5].copy_from_slice(&r.to_le_bytes());
            }
            i += 5;
        } else {
            i += 1;
        }
    }
}

/// AArch64 BL imm26 (word-aligned within the file).
fn arm64(buf: &mut [u8], enc: bool) {
    let n = buf.len();
    let mut i = 0usize;
    while i + 4 <= n {
        let w = u32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]);
        if w & 0xFC00_0000 == 0x9400_0000 {
            let imm = w & 0x03FF_FFFF;
            let pc = (i as u32) >> 2;
            let ni = if enc {
                imm.wrapping_add(pc)
            } else {
                imm.wrapping_sub(pc)
            } & 0x03FF_FFFF;
            buf[i..i + 4].copy_from_slice(&(0x9400_0000 | ni).to_le_bytes());
        }
        i += 4;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_random() {
        let mut s = 12345u64;
        for len in [0usize, 1, 4, 5, 7, 100, 4096, 100_003] {
            let data: Vec<u8> = (0..len)
                .map(|_| {
                    s = s
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let b = (s >> 56) as u8;
                    match b {
                        0..=39 => 0xE8,
                        40..=59 => 0x94,
                        60..=69 => 0x48,
                        70..=79 => 0x8B,
                        80..=89 => 0x05,
                        90..=94 => 0x0F,
                        95..=99 => 0x66,
                        _ => b,
                    }
                })
                .collect();
            for xf in [XF_X86, XF_ARM64, XF_X86_64, XF_X86_64_SPLIT] {
                let mut d = data.clone();
                encode(xf, &mut d);
                decode(xf, &mut d);
                assert_eq!(d, data, "xf={xf} len={len}");
            }
        }
    }

    #[test]
    fn conv_range_matches_conv24_and_inverts() {
        let mut s = 99u64;
        for _ in 0..200_000 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let v = (s >> 32) as u32;
            let pos = (s as u32) >> 3;
            let mut b4 = v.to_le_bytes().to_vec();
            conv24(&mut b4, 0, pos as usize, true);
            assert_eq!(conv_range(v, pos, 23, true), u32::from_le_bytes([b4[0], b4[1], b4[2], b4[3]]));
            for b in [23, 24, 26, 30, 31] {
                let w = conv_range(v, pos, b, true);
                assert_eq!(conv_range(w, pos, b, false), v, "b={b}");
            }
        }
        assert_eq!(range_bits(0), 23);
        assert_eq!(range_bits(1 << 23), 23);
        assert_eq!(range_bits((1 << 23) + 1), 24);
        assert_eq!(range_bits(48_051_472), 26);
        assert_eq!(range_bits(usize::MAX), 31);
    }

    /// Short buffers made mostly of pattern bytes: every combination of prefixes,
    /// REX bytes, opcodes, and ModRM values near the end of the data.
    #[test]
    fn split_roundtrip_dense_patterns() {
        let pool: Vec<u8> = {
            let mut p = vec![0xE8, 0xE9, 0x66, 0xF2, 0xF3, 0x0F, 0x05, 0x0D, 0x15, 0x45, 0x00, 0xFF, 0x80, 0x7F];
            p.extend(0x40..=0x4F);
            p.extend((0..=255u8).filter(|&b| RIP_OP1[b as usize] || RIP_OP2[b as usize]));
            p
        };
        let mut s = 7u64;
        let mut next = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 33) as usize
        };
        for _ in 0..300_000 {
            let len = 5 + next() % 40;
            let data: Vec<u8> = (0..len)
                .map(|_| if next() % 5 == 0 { next() as u8 } else { pool[next() % pool.len()] })
                .collect();
            let mut d = data.clone();
            x86_64_split_encode(&mut d);
            x86_64_split_decode(&mut d);
            assert_eq!(d, data);
        }
    }
}
