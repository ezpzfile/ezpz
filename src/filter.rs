//! Reversible, length-preserving file transforms for machine code (SPEC.md §9).
//! They turn relative branch targets into absolute ones so repeated calls to the
//! same function become identical byte patterns the compressor can find.

pub const XF_NONE: u8 = 0;
pub const XF_X86: u8 = 1;
pub const XF_ARM64: u8 = 2;
pub const XF_MAX: u8 = 2;

pub fn encode(xf: u8, buf: &mut [u8]) {
    match xf {
        XF_X86 => x86(buf, true),
        XF_ARM64 => arm64(buf, true),
        _ => {}
    }
}

pub fn decode(xf: u8, buf: &mut [u8]) {
    match xf {
        XF_X86 => x86(buf, false),
        XF_ARM64 => arm64(buf, false),
        _ => {}
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
                    if b < 40 {
                        0xE8
                    } else if b < 60 {
                        0x94
                    } else {
                        b
                    }
                })
                .collect();
            for xf in [XF_X86, XF_ARM64] {
                let mut d = data.clone();
                encode(xf, &mut d);
                decode(xf, &mut d);
                assert_eq!(d, data, "xf={xf} len={len}");
            }
        }
    }
}
