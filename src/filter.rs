//! Reversible, length-preserving file transforms for machine code (SPEC.md §9).
//! They turn relative branch targets into absolute ones so repeated calls to the
//! same function become identical byte patterns the compressor can find.

pub const XF_NONE: u8 = 0;
pub const XF_X86: u8 = 1;
pub const XF_ARM64: u8 = 2;
pub const XF_X86_64: u8 = 3;
pub const XF_MAX: u8 = 3;

pub fn encode(xf: u8, buf: &mut [u8]) {
    match xf {
        XF_X86 => x86(buf, true),
        XF_ARM64 => arm64(buf, true),
        XF_X86_64 => x86_64(buf, true),
        _ => {}
    }
}

pub fn decode(xf: u8, buf: &mut [u8]) {
    match xf {
        XF_X86 => x86(buf, false),
        XF_ARM64 => arm64(buf, false),
        XF_X86_64 => x86_64(buf, false),
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
            for xf in [XF_X86, XF_ARM64, XF_X86_64] {
                let mut d = data.clone();
                encode(xf, &mut d);
                decode(xf, &mut d);
                assert_eq!(d, data, "xf={xf} len={len}");
            }
        }
    }
}
