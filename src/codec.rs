//! Block codecs: store, zstd, LZMA2 (raw), brain, brain-fast, zstd-primed (SPEC.md §6).

use crate::format::*;
use anyhow::{Result, anyhow, bail, ensure};
use liblzma::stream::{Action, Filters, LzmaOptions, Status, Stream};

#[derive(Clone, Copy, Debug)]
pub enum Plan {
    Store,
    Zstd(i32),
    Lzma2(u32),
    /// Try zstd and LZMA2, keep the smaller.
    Auto {
        zstd: i32,
        lzma: u32,
    },
    Brain(crate::brain::Profile),
}

/// zstd blocks up to this size are also tried with the priming data as a dictionary.
const ZSTD_PRIME_TRIAL_MAX: usize = 1 << 20;

/// zstd for a data block: small blocks are compressed with and without the priming
/// dictionary (codec 5), and the smaller result is kept (codec 1 on a tie).
fn zstd_block(raw: &[u8], level: i32) -> Result<(u8, Vec<u8>)> {
    let plain = zstd_compress(raw, level, None)?;
    if raw.len() <= ZSTD_PRIME_TRIAL_MAX {
        let primed = zstd_compress(raw, level, Some(crate::brain::PRIME))?;
        if primed.len() < plain.len() {
            return Ok((CODEC_ZSTD_PRIMED, primed));
        }
    }
    Ok((CODEC_ZSTD, plain))
}

/// Index frames (block table and catalog) may only use codecs 0, 1 and 2.
pub fn compress_index(raw: &[u8]) -> Result<(u8, Vec<u8>)> {
    let out = zstd_compress(raw, 19, None)?;
    Ok(if out.len() >= raw.len() { (CODEC_STORE, raw.to_vec()) } else { (CODEC_ZSTD, out) })
}

pub fn compress(raw: &[u8], plan: Plan) -> Result<(u8, Vec<u8>)> {
    let (codec, out) = match plan {
        Plan::Store => return Ok((CODEC_STORE, raw.to_vec())),
        Plan::Zstd(l) => zstd_block(raw, l)?,
        Plan::Lzma2(p) => (CODEC_LZMA2, lzma2_compress(raw, p)?),
        Plan::Auto { zstd, lzma } => {
            let (a, b) = rayon::join(|| zstd_block(raw, zstd), || lzma2_compress(raw, lzma));
            let (a, b) = (a?, b?);
            if b.len() < a.1.len() {
                (CODEC_LZMA2, b)
            } else {
                a
            }
        }
        Plan::Brain(profile) => (
            match profile {
                crate::brain::Profile::Full => CODEC_BRAIN,
                _ => CODEC_BRAIN_FAST,
            },
            crate::brain::compress(raw, profile),
        ),
    };
    if out.len() >= raw.len() {
        Ok((CODEC_STORE, raw.to_vec()))
    } else {
        Ok((codec, out))
    }
}

pub fn decompress(codec: u8, stored: &[u8], raw_len: usize) -> Result<Vec<u8>> {
    let out = match codec {
        CODEC_STORE => {
            ensure!(stored.len() == raw_len, "stored block size mismatch");
            stored.to_vec()
        }
        CODEC_ZSTD => zstd_decompress(stored, raw_len, None)?,
        CODEC_ZSTD_PRIMED => zstd_decompress(stored, raw_len, Some(crate::brain::PRIME))?,
        CODEC_LZMA2 => lzma2_decompress(stored, raw_len)?,
        CODEC_BRAIN => crate::brain::decompress(stored, raw_len, crate::brain::Profile::Full)?,
        CODEC_BRAIN_FAST => crate::brain::decompress(stored, raw_len, crate::brain::Profile::Fast)?,
        c => bail!("unknown codec {c} (archive made by a newer version?)"),
    };
    ensure!(out.len() == raw_len, "decompressed size mismatch");
    Ok(out)
}

fn window_log_for(n: usize) -> u32 {
    let mut w = 10;
    while (1usize << w) < n && w < 28 {
        w += 1;
    }
    w
}

fn zstd_err(code: usize) -> anyhow::Error {
    anyhow!("zstd: {}", zstd::zstd_safe::get_error_name(code))
}

/// `dict` is loaded as a raw content dictionary (the priming data has no dictionary header).
/// Parameters are set before the dictionary, as zstd requires.
fn zstd_compress(raw: &[u8], level: i32, dict: Option<&[u8]>) -> Result<Vec<u8>> {
    use zstd::zstd_safe::{CCtx, CParameter};
    let mut c = CCtx::create();
    c.set_parameter(CParameter::CompressionLevel(level)).map_err(zstd_err)?;
    c.set_parameter(CParameter::WindowLog(window_log_for(raw.len()))).map_err(zstd_err)?;
    if level >= 4 {
        c.set_parameter(CParameter::EnableLongDistanceMatching(true)).map_err(zstd_err)?;
    }
    c.set_parameter(CParameter::ChecksumFlag(false)).map_err(zstd_err)?;
    c.set_parameter(CParameter::ContentSizeFlag(true)).map_err(zstd_err)?;
    if let Some(d) = dict {
        c.load_dictionary(d).map_err(zstd_err)?;
    }
    let mut out = Vec::with_capacity(zstd::zstd_safe::compress_bound(raw.len()));
    c.compress2(&mut out, raw).map_err(zstd_err)?;
    Ok(out)
}

fn zstd_decompress(stored: &[u8], raw_len: usize, dict: Option<&[u8]>) -> Result<Vec<u8>> {
    use zstd::zstd_safe::{DCtx, DParameter};
    let mut d = DCtx::create();
    d.set_parameter(DParameter::WindowLogMax(28)).map_err(zstd_err)?;
    if let Some(x) = dict {
        d.load_dictionary(x).map_err(zstd_err)?;
    }
    let mut out = Vec::with_capacity(raw_len);
    d.decompress(&mut out, stored).map_err(zstd_err)?;
    Ok(out)
}

/// LZMA2 dictionary size encoded by property byte `p` (xz format spec §5.3.1).
pub fn lzma2_dict(p: u8) -> u32 {
    if p >= 40 {
        u32::MAX
    } else {
        (2 | (p as u32 & 1)) << (p as u32 / 2 + 11)
    }
}
pub const LZMA2_MAX_PROP: u8 = 32; // dictionary 256 MiB

fn lzma2_compress(raw: &[u8], preset: u32) -> Result<Vec<u8>> {
    let mut opts = LzmaOptions::new_preset(preset)?;
    let need = raw.len().max(4096) as u64;
    let mut p = 0u8;
    while (lzma2_dict(p) as u64) < need && p < LZMA2_MAX_PROP {
        p += 1;
    }
    opts.dict_size(lzma2_dict(p));
    let mut f = Filters::new();
    f.lzma2(&opts);
    let mut s = Stream::new_raw_encoder(&f)?;
    let mut out = Vec::with_capacity(raw.len() / 3 + 4096);
    out.push(p);
    loop {
        if out.len() == out.capacity() {
            out.reserve(out.capacity().max(4096));
        }
        let consumed = s.total_in() as usize;
        let st = s.process_vec(&raw[consumed..], &mut out, Action::Finish)?;
        if matches!(st, Status::StreamEnd) {
            break;
        }
    }
    Ok(out)
}

fn lzma2_decompress(stored: &[u8], raw_len: usize) -> Result<Vec<u8>> {
    ensure!(!stored.is_empty(), "empty LZMA2 block");
    let p = stored[0];
    ensure!(p <= LZMA2_MAX_PROP, "LZMA2 dictionary larger than allowed");
    let mut f = Filters::new();
    f.lzma2_properties(&[p])?;
    let mut s = Stream::new_raw_decoder(&f)?;
    let input = &stored[1..];
    let mut out = Vec::with_capacity(raw_len + 1);
    loop {
        let (in0, out0) = (s.total_in() as usize, out.len());
        let st = s.process_vec(&input[in0..], &mut out, Action::Run)?;
        if matches!(st, Status::StreamEnd) {
            break;
        }
        ensure!(out.len() <= raw_len, "LZMA2 output larger than declared");
        ensure!(
            s.total_in() as usize != in0 || out.len() != out0,
            "LZMA2 stream truncated"
        );
    }
    ensure!(
        s.total_in() as usize == input.len(),
        "trailing bytes after LZMA2 stream"
    );
    Ok(out)
}

/// Quick incompressibility probe used by the classifier.
pub fn looks_incompressible(sample: &[u8]) -> bool {
    if sample.len() < 512 {
        return false;
    }
    match zstd::bulk::compress(sample, 1) {
        Ok(c) => c.len() * 100 >= sample.len() * 97,
        Err(_) => false,
    }
}
