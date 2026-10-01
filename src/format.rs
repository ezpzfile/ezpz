//! Byte-level layout of the .ezpz container (see SPEC.md).

use anyhow::{Result, bail, ensure};

pub const MAGIC: [u8; 8] = [0x89, b'E', b'Z', b'P', b'Z', 0x0D, 0x0A, 0x1A];
pub const END_MAGIC: [u8; 8] = *b"EZPZEND\x1a";
pub const BLOCK_MAGIC: [u8; 4] = *b"EZBK";
pub const TABLE_MAGIC: [u8; 4] = *b"EZBT";
pub const CATALOG_MAGIC: [u8; 4] = *b"EZCT";
pub const SIG_MAGIC: [u8; 4] = *b"EZSG";

pub const VERSION_MAJOR: u8 = 1;
pub const VERSION_MINOR: u8 = 0;

pub const HEADER_FIXED_LEN: usize = 32;
pub const FRAME_HEADER_LEN: usize = 16;
pub const TRAILER_LEN: usize = 64;
pub const SIG_SECTION_LEN: usize = 104;

pub const MAX_HEADER_EXT: u32 = 65_536;
pub const MAX_BLOCK_RAW: u32 = 256 << 20;
pub const MAX_INDEX_RAW: u32 = 1 << 30;

// Header flags
pub const HF_ENCRYPTED: u16 = 1 << 0;
pub const HF_SIGNED: u16 = 1 << 1;
const HF_KNOWN: u16 = HF_ENCRYPTED | HF_SIGNED;

// Frame flags
pub const FF_ENCRYPTED: u8 = 1 << 0;
const FF_KNOWN: u8 = FF_ENCRYPTED;

// Codecs
pub const CODEC_STORE: u8 = 0;
pub const CODEC_ZSTD: u8 = 1;
pub const CODEC_LZMA2: u8 = 2;
pub const CODEC_BRAIN: u8 = 3;
pub const CODEC_BRAIN_FAST: u8 = 4;

pub fn codec_name(c: u8) -> &'static str {
    match c {
        CODEC_STORE => "store",
        CODEC_ZSTD => "zstd",
        CODEC_LZMA2 => "lzma2",
        CODEC_BRAIN => "brain",
        CODEC_BRAIN_FAST => "brain-fast",
        _ => "unknown",
    }
}

// Trailer flags
pub const TF_SIGNATURE: u32 = 1 << 0;
const TF_KNOWN: u32 = TF_SIGNATURE;

// Header extension records (bit 7 = critical: unknown critical records must be rejected)
pub const REC_CRITICAL: u8 = 0x80;
pub const REC_ENCRYPTION: u8 = 0x81;
pub const CIPHER_XCHACHA20POLY1305: u8 = 1;
pub const KDF_ARGON2ID: u8 = 1;

pub const SIG_ED25519: u8 = 1;

/// Domain-separation prefix of the signed message.
pub const SIG_CONTEXT: &[u8] = b"EZPZ-v1 signature\0";

// ---------------------------------------------------------------- varints

pub fn put_uv(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

pub fn put_sv(out: &mut Vec<u8>, v: i64) {
    put_uv(out, ((v << 1) ^ (v >> 63)) as u64);
}

pub struct ByteReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        ByteReader { buf, pos: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    pub fn u8(&mut self) -> Result<u8> {
        ensure!(self.pos < self.buf.len(), "unexpected end of index data");
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(b)
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(n <= self.remaining(), "unexpected end of index data");
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn uv(&mut self) -> Result<u64> {
        let mut r = 0u64;
        let mut shift = 0u32;
        loop {
            let b = self.u8()?;
            ensure!(shift < 64 && !(shift == 63 && b > 1), "varint overflow");
            r |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                ensure!(b != 0 || shift == 0, "non-minimal varint");
                return Ok(r);
            }
            shift += 7;
        }
    }
    /// Varint that must fit in u32.
    pub fn uv32(&mut self) -> Result<u32> {
        let v = self.uv()?;
        ensure!(v <= u32::MAX as u64, "value out of range");
        Ok(v as u32)
    }
    /// A count of items, each needing at least `min_bytes` bytes - guards allocations.
    pub fn count(&mut self, min_bytes: usize) -> Result<usize> {
        let v = self.uv()?;
        ensure!(
            v as u128 * min_bytes.max(1) as u128 <= self.remaining() as u128,
            "item count larger than remaining data"
        );
        Ok(v as usize)
    }
    pub fn sv(&mut self) -> Result<i64> {
        let u = self.uv()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }
    pub fn finish(&self) -> Result<()> {
        ensure!(self.pos == self.buf.len(), "trailing bytes in index data");
        Ok(())
    }
}

fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn le64(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[..8].try_into().unwrap())
}

// ---------------------------------------------------------------- header

#[derive(Clone, Debug)]
pub struct Encryption {
    pub cipher: u8,
    pub kdf: u8,
    pub salt: [u8; 16],
    pub m_kib: u32,
    pub t_cost: u32,
    pub p_lanes: u32,
}

#[derive(Clone, Debug)]
pub struct Header {
    pub version_minor: u8,
    pub flags: u16,
    pub archive_id: [u8; 16],
    pub encryption: Option<Encryption>,
    /// Exact bytes as they appear in the file (fixed part + extension).
    pub bytes: Vec<u8>,
}

impl Header {
    pub fn build(flags: u16, archive_id: [u8; 16], encryption: Option<Encryption>) -> Header {
        let mut ext = Vec::new();
        if let Some(e) = &encryption {
            let mut v = Vec::new();
            v.push(e.cipher);
            v.push(e.kdf);
            v.extend_from_slice(&e.salt);
            v.extend_from_slice(&e.m_kib.to_le_bytes());
            v.extend_from_slice(&e.t_cost.to_le_bytes());
            v.extend_from_slice(&e.p_lanes.to_le_bytes());
            ext.push(REC_ENCRYPTION);
            ext.extend_from_slice(&(v.len() as u16).to_le_bytes());
            ext.extend_from_slice(&v);
        }
        let mut b = Vec::with_capacity(HEADER_FIXED_LEN + ext.len());
        b.extend_from_slice(&MAGIC);
        b.push(VERSION_MAJOR);
        b.push(VERSION_MINOR);
        b.extend_from_slice(&flags.to_le_bytes());
        b.extend_from_slice(&(ext.len() as u32).to_le_bytes());
        b.extend_from_slice(&archive_id);
        b.extend_from_slice(&ext);
        Header {
            version_minor: VERSION_MINOR,
            flags,
            archive_id,
            encryption,
            bytes: b,
        }
    }

    /// Validate the fixed 32-byte part and return the extension length.
    pub fn ext_len(fixed: &[u8]) -> Result<u32> {
        ensure!(fixed.len() >= HEADER_FIXED_LEN, "file too short");
        ensure!(fixed[..8] == MAGIC, "not an .ezpz archive (bad magic)");
        ensure!(
            fixed[8] == VERSION_MAJOR,
            "unsupported format version {}.{} (this tool reads {}.x)",
            fixed[8],
            fixed[9],
            VERSION_MAJOR
        );
        let flags = le16(&fixed[10..]);
        ensure!(
            flags & !HF_KNOWN == 0,
            "archive uses unknown header flags {flags:#06x}"
        );
        let ext_len = le32(&fixed[12..]);
        ensure!(ext_len <= MAX_HEADER_EXT, "header extension too large");
        Ok(ext_len)
    }

    pub fn parse(bytes: Vec<u8>) -> Result<Header> {
        let ext_len = Header::ext_len(&bytes)? as usize;
        ensure!(
            bytes.len() == HEADER_FIXED_LEN + ext_len,
            "header length mismatch"
        );
        let flags = le16(&bytes[10..]);
        let archive_id: [u8; 16] = bytes[16..32].try_into().unwrap();
        let mut encryption = None;
        let mut seen_types = [false; 256];
        let mut p = HEADER_FIXED_LEN;
        while p < bytes.len() {
            ensure!(p + 3 <= bytes.len(), "truncated header record");
            let ty = bytes[p];
            let len = le16(&bytes[p + 1..]) as usize;
            p += 3;
            ensure!(p + len <= bytes.len(), "truncated header record");
            let v = &bytes[p..p + len];
            p += len;
            ensure!(
                !seen_types[ty as usize],
                "duplicate header record {ty:#04x}"
            );
            seen_types[ty as usize] = true;
            match ty {
                REC_ENCRYPTION => {
                    ensure!(v.len() >= 30, "bad encryption record");
                    encryption = Some(Encryption {
                        cipher: v[0],
                        kdf: v[1],
                        salt: v[2..18].try_into().unwrap(),
                        m_kib: le32(&v[18..]),
                        t_cost: le32(&v[22..]),
                        p_lanes: le32(&v[26..]),
                    });
                }
                t if t & REC_CRITICAL != 0 => {
                    bail!("archive needs unknown feature (record {t:#04x})")
                }
                _ => {} // unknown non-critical record: ignore
            }
        }
        ensure!(
            (flags & HF_ENCRYPTED != 0) == encryption.is_some(),
            "encryption flag and encryption record disagree"
        );
        Ok(Header {
            version_minor: bytes[9],
            flags,
            archive_id,
            encryption,
            bytes,
        })
    }

    pub fn encrypted(&self) -> bool {
        self.flags & HF_ENCRYPTED != 0
    }
    pub fn signed(&self) -> bool {
        self.flags & HF_SIGNED != 0
    }
}

// ---------------------------------------------------------------- frames

#[derive(Clone, Copy, Debug)]
pub struct FrameHeader {
    pub magic: [u8; 4],
    pub codec: u8,
    pub flags: u8,
    pub raw_len: u32,
    pub stored_len: u32,
}

impl FrameHeader {
    pub fn encode(&self) -> [u8; FRAME_HEADER_LEN] {
        let mut b = [0u8; FRAME_HEADER_LEN];
        b[..4].copy_from_slice(&self.magic);
        b[4] = self.codec;
        b[5] = self.flags;
        b[8..12].copy_from_slice(&self.raw_len.to_le_bytes());
        b[12..16].copy_from_slice(&self.stored_len.to_le_bytes());
        b
    }
    pub fn decode(b: &[u8], magic: [u8; 4]) -> Result<FrameHeader> {
        ensure!(b.len() >= FRAME_HEADER_LEN, "truncated frame header");
        ensure!(
            b[..4] == magic,
            "bad frame magic (expected {:?})",
            String::from_utf8_lossy(&magic)
        );
        ensure!(b[6] == 0 && b[7] == 0, "reserved frame bytes must be zero");
        let flags = b[5];
        ensure!(flags & !FF_KNOWN == 0, "unknown frame flags {flags:#04x}");
        Ok(FrameHeader {
            magic,
            codec: b[4],
            flags,
            raw_len: le32(&b[8..]),
            stored_len: le32(&b[12..]),
        })
    }
    pub fn encrypted(&self) -> bool {
        self.flags & FF_ENCRYPTED != 0
    }
}

// ---------------------------------------------------------------- trailer

#[derive(Clone, Debug)]
pub struct Trailer {
    pub index_offset: u64,
    pub archive_len: u64,
    pub root_hash: [u8; 32],
    pub flags: u32,
}

impl Trailer {
    pub fn encode(&self) -> [u8; TRAILER_LEN] {
        let mut b = [0u8; TRAILER_LEN];
        b[0..8].copy_from_slice(&self.index_offset.to_le_bytes());
        b[8..16].copy_from_slice(&self.archive_len.to_le_bytes());
        b[16..48].copy_from_slice(&self.root_hash);
        b[48..52].copy_from_slice(&self.flags.to_le_bytes());
        let check = blake3::hash(&b[..52]);
        b[52..56].copy_from_slice(&check.as_bytes()[..4]);
        b[56..64].copy_from_slice(&END_MAGIC);
        b
    }
    pub fn decode(b: &[u8]) -> Result<Trailer> {
        ensure!(b.len() == TRAILER_LEN, "bad trailer length");
        ensure!(
            b[56..64] == END_MAGIC,
            "trailer not found (file truncated or not an .ezpz archive)"
        );
        let check = blake3::hash(&b[..52]);
        ensure!(
            b[52..56] == check.as_bytes()[..4],
            "trailer checksum mismatch (file damaged)"
        );
        let flags = le32(&b[48..]);
        ensure!(flags & !TF_KNOWN == 0, "unknown trailer flags {flags:#x}");
        Ok(Trailer {
            index_offset: le64(&b[0..]),
            archive_len: le64(&b[8..]),
            root_hash: b[16..48].try_into().unwrap(),
            flags,
        })
    }
}

// ---------------------------------------------------------------- signature

#[derive(Clone, Debug)]
pub struct SigSection {
    pub algo: u8,
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}

impl SigSection {
    pub fn encode(&self) -> [u8; SIG_SECTION_LEN] {
        let mut b = [0u8; SIG_SECTION_LEN];
        b[..4].copy_from_slice(&SIG_MAGIC);
        b[4] = self.algo;
        b[8..40].copy_from_slice(&self.public_key);
        b[40..104].copy_from_slice(&self.signature);
        b
    }
    pub fn decode(b: &[u8]) -> Result<SigSection> {
        ensure!(
            b.len() == SIG_SECTION_LEN && b[..4] == SIG_MAGIC,
            "bad signature section"
        );
        ensure!(b[4] == SIG_ED25519, "unknown signature algorithm {}", b[4]);
        ensure!(
            b[5..8] == [0, 0, 0],
            "reserved signature bytes must be zero"
        );
        Ok(SigSection {
            algo: b[4],
            public_key: b[8..40].try_into().unwrap(),
            signature: b[40..104].try_into().unwrap(),
        })
    }
}

/// Root of the integrity chain and the archive's fingerprint:
/// BLAKE3(header bytes || index region). The index region holds every block's hash,
/// so this one value commits to every byte before the signature/trailer.
pub fn root_hash(header_bytes: &[u8], index_region: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(header_bytes);
    h.update(index_region);
    *h.finalize().as_bytes()
}

pub fn signed_message(digest: &[u8; 32]) -> Vec<u8> {
    let mut m = SIG_CONTEXT.to_vec();
    m.extend_from_slice(digest);
    m
}

// ---------------------------------------------------------------- paths

/// Rules every stored path must follow (relative, '/'-separated, no traversal).
pub fn validate_path(p: &str) -> Result<()> {
    ensure!(!p.is_empty() && p.len() <= 4096, "bad path length");
    ensure!(
        !p.contains('\0') && !p.contains('\\'),
        "path contains NUL or backslash: {p:?}"
    );
    ensure!(!p.starts_with('/'), "absolute path not allowed: {p:?}");
    for (i, comp) in p.split('/').enumerate() {
        ensure!(
            !comp.is_empty() && comp != "." && comp != "..",
            "unsafe path component in {p:?}"
        );
        if i == 0 {
            ensure!(
                !comp.contains(':'),
                "':' not allowed in the first path component (drive letters): {p:?}"
            );
        }
    }
    Ok(())
}
