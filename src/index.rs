//! Block table (never encrypted) and catalog (optionally encrypted). See SPEC.md §7-8.

use crate::format::*;
use anyhow::{Result, ensure};

pub const TABLE_VERSION: u64 = 1;
pub const CATALOG_VERSION: u64 = 1;

pub const ET_FILE: u8 = 0;
pub const ET_DIR: u8 = 1;
pub const ET_SYMLINK: u8 = 2;

#[derive(Clone, Debug)]
pub struct BlockEntry {
    pub raw_len: u32,
    pub stored_len: u32,
    /// BLAKE3-256 of the whole frame (16-byte frame header + stored bytes).
    pub hash: [u8; 32],
}

#[derive(Clone, Debug, Default)]
pub struct BlockTable {
    pub blocks: Vec<BlockEntry>,
}

impl BlockTable {
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        put_uv(&mut o, TABLE_VERSION);
        put_uv(&mut o, self.blocks.len() as u64);
        for b in &self.blocks {
            put_uv(&mut o, b.raw_len as u64);
        }
        for b in &self.blocks {
            put_uv(&mut o, b.stored_len as u64);
        }
        for b in &self.blocks {
            o.extend_from_slice(&b.hash);
        }
        o
    }

    pub fn decode(buf: &[u8]) -> Result<BlockTable> {
        let mut r = ByteReader::new(buf);
        ensure!(r.uv()? == TABLE_VERSION, "unsupported block table version");
        let n = r.count(34)?;
        let mut raw = Vec::with_capacity(n);
        for _ in 0..n {
            let v = r.uv32()?;
            ensure!(v >= 1 && v <= MAX_BLOCK_RAW, "block raw size out of range");
            raw.push(v);
        }
        let mut stored = Vec::with_capacity(n);
        for _ in 0..n {
            stored.push(r.uv32()?);
        }
        let mut blocks = Vec::with_capacity(n);
        for i in 0..n {
            blocks.push(BlockEntry {
                raw_len: raw[i],
                stored_len: stored[i],
                hash: r.bytes(32)?.try_into().unwrap(),
            });
        }
        r.finish()?;
        Ok(BlockTable { blocks })
    }

    /// Offsets of each block frame; blocks are contiguous right after the header.
    pub fn offsets(&self, header_len: u64) -> (Vec<u64>, u64) {
        let mut offs = Vec::with_capacity(self.blocks.len());
        let mut o = header_len;
        for b in &self.blocks {
            offs.push(o);
            o += FRAME_HEADER_LEN as u64 + b.stored_len as u64;
        }
        (offs, o)
    }
}

#[derive(Clone, Debug)]
pub enum Kind {
    File {
        transform: u8,
        chunks: Vec<u32>,
        hash: Vec<u8>,
    },
    Dir,
    Symlink {
        target: String,
    },
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: String,
    pub mode: u32,
    pub mtime_s: i64,
    pub mtime_ns: u32,
    pub kind: Kind,
}

#[derive(Clone, Debug)]
pub struct Catalog {
    pub hash_len: u8,
    pub created: i64,
    pub creator: String,
    /// Length of every chunk, grouped per block (chunk ids are assigned in this order).
    pub chunk_lens: Vec<Vec<u32>>,
    /// Sorted by path (bytewise), unique.
    pub entries: Vec<Entry>,
}

/// Where each chunk lives, derived from the catalog.
pub struct ChunkMap {
    pub block: Vec<u32>,
    pub offset: Vec<u32>,
    pub len: Vec<u32>,
}

impl Catalog {
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        put_uv(&mut o, CATALOG_VERSION);
        o.push(self.hash_len);
        put_sv(&mut o, self.created);
        put_uv(&mut o, self.creator.len() as u64);
        o.extend_from_slice(self.creator.as_bytes());

        put_uv(&mut o, self.chunk_lens.len() as u64);
        for b in &self.chunk_lens {
            put_uv(&mut o, b.len() as u64);
        }
        for b in &self.chunk_lens {
            for &l in b {
                put_uv(&mut o, l as u64);
            }
        }

        let es = &self.entries;
        put_uv(&mut o, es.len() as u64);
        let mut prev: &[u8] = &[];
        for e in es {
            let p = e.path.as_bytes();
            let shared = prev.iter().zip(p).take_while(|(a, b)| a == b).count();
            put_uv(&mut o, shared as u64);
            put_uv(&mut o, (p.len() - shared) as u64);
            o.extend_from_slice(&p[shared..]);
            prev = p;
        }
        for e in es {
            o.push(match e.kind {
                Kind::File { .. } => ET_FILE,
                Kind::Dir => ET_DIR,
                Kind::Symlink { .. } => ET_SYMLINK,
            });
        }
        for e in es {
            put_uv(&mut o, e.mode as u64);
        }
        let mut prev_t = 0i64;
        for e in es {
            put_sv(&mut o, e.mtime_s.wrapping_sub(prev_t));
            prev_t = e.mtime_s;
        }
        for e in es {
            put_uv(&mut o, e.mtime_ns as u64);
        }
        let files: Vec<_> = es
            .iter()
            .filter_map(|e| match &e.kind {
                Kind::File {
                    transform,
                    chunks,
                    hash,
                } => Some((*transform, chunks, hash)),
                _ => None,
            })
            .collect();
        for f in &files {
            o.push(f.0);
        }
        for f in &files {
            put_uv(&mut o, f.1.len() as u64);
        }
        let mut prev_ref: i64 = -1;
        for f in &files {
            for &c in f.1.iter() {
                put_sv(&mut o, c as i64 - (prev_ref + 1));
                prev_ref = c as i64;
            }
        }
        for f in &files {
            o.extend_from_slice(&f.2[..self.hash_len as usize]);
        }
        for e in es {
            if let Kind::Symlink { target } = &e.kind {
                put_uv(&mut o, target.len() as u64);
                o.extend_from_slice(target.as_bytes());
            }
        }
        o
    }

    pub fn decode(buf: &[u8], table: &BlockTable) -> Result<Catalog> {
        let mut r = ByteReader::new(buf);
        ensure!(r.uv()? == CATALOG_VERSION, "unsupported catalog version");
        let hash_len = r.u8()?;
        ensure!(
            matches!(hash_len, 0 | 16 | 32),
            "bad file hash length {hash_len}"
        );
        let created = r.sv()?;
        let cl = r.count(1)?;
        let creator = String::from_utf8_lossy(r.bytes(cl)?).into_owned();

        let nb = r.count(1)?;
        ensure!(
            nb == table.blocks.len(),
            "catalog and block table disagree on block count"
        );
        let mut counts = Vec::with_capacity(nb);
        for _ in 0..nb {
            counts.push(r.count(1)?);
        }
        let mut chunk_lens = Vec::with_capacity(nb);
        let mut total_chunks: u64 = 0;
        for (bi, &c) in counts.iter().enumerate() {
            let mut v = Vec::with_capacity(c);
            let mut sum = 0u64;
            for _ in 0..c {
                let l = r.uv32()?;
                ensure!(l > 0, "zero-length chunk");
                sum += l as u64;
                v.push(l);
            }
            ensure!(
                sum == table.blocks[bi].raw_len as u64,
                "chunk sizes do not add up to block {bi} size"
            );
            total_chunks += c as u64;
            chunk_lens.push(v);
        }
        ensure!(total_chunks <= u32::MAX as u64, "too many chunks");

        let ne = r.count(3)?;
        let mut paths: Vec<String> = Vec::with_capacity(ne);
        let mut prev: Vec<u8> = Vec::new();
        for _ in 0..ne {
            let shared = r.uv()? as usize;
            let slen = r.count(1)?;
            ensure!(shared <= prev.len(), "bad path prefix");
            let mut p = prev[..shared].to_vec();
            p.extend_from_slice(r.bytes(slen)?);
            ensure!(
                p > prev || paths.is_empty(),
                "entries not strictly sorted / duplicate path"
            );
            let s =
                String::from_utf8(p.clone()).map_err(|_| anyhow::anyhow!("path is not UTF-8"))?;
            validate_path(&s)?;
            paths.push(s);
            prev = p;
        }
        let mut types = Vec::with_capacity(ne);
        for _ in 0..ne {
            let t = r.u8()?;
            ensure!(t <= ET_SYMLINK, "unknown entry type {t}");
            types.push(t);
        }
        let mut modes = Vec::with_capacity(ne);
        for _ in 0..ne {
            modes.push((r.uv()? & 0o7777) as u32);
        }
        let mut mt = Vec::with_capacity(ne);
        let mut prev_t = 0i64;
        for _ in 0..ne {
            prev_t = prev_t.wrapping_add(r.sv()?);
            mt.push(prev_t);
        }
        let mut mns = Vec::with_capacity(ne);
        for _ in 0..ne {
            let v = r.uv32()?;
            ensure!(v < 1_000_000_000, "bad nanoseconds");
            mns.push(v);
        }
        let nf = types.iter().filter(|&&t| t == ET_FILE).count();
        let mut transforms = Vec::with_capacity(nf);
        for _ in 0..nf {
            let t = r.u8()?;
            ensure!(t <= crate::filter::XF_MAX, "unknown file transform {t}");
            transforms.push(t);
        }
        let mut nch = Vec::with_capacity(nf);
        for _ in 0..nf {
            nch.push(r.count(1)?);
        }
        let mut refs: Vec<Vec<u32>> = Vec::with_capacity(nf);
        let mut prev_ref: i64 = -1;
        for &n in &nch {
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                let c = (prev_ref + 1)
                    .checked_add(r.sv()?)
                    .ok_or_else(|| anyhow::anyhow!("bad chunk ref"))?;
                ensure!(
                    c >= 0 && (c as u64) < total_chunks,
                    "chunk reference out of range"
                );
                v.push(c as u32);
                prev_ref = c;
            }
            refs.push(v);
        }
        let mut hashes = Vec::with_capacity(nf);
        for _ in 0..nf {
            hashes.push(r.bytes(hash_len as usize)?.to_vec());
        }
        let mut entries = Vec::with_capacity(ne);
        let mut fi = 0;
        let mut refs_it = refs.into_iter();
        let mut hash_it = hashes.into_iter();
        for i in 0..ne {
            let kind = match types[i] {
                ET_FILE => {
                    let k = Kind::File {
                        transform: transforms[fi],
                        chunks: refs_it.next().unwrap(),
                        hash: hash_it.next().unwrap(),
                    };
                    fi += 1;
                    k
                }
                ET_DIR => Kind::Dir,
                _ => {
                    let l = r.count(1)?;
                    let t = String::from_utf8(r.bytes(l)?.to_vec())
                        .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
                    ensure!(!t.is_empty() && !t.contains('\0'), "bad symlink target");
                    Kind::Symlink { target: t }
                }
            };
            entries.push(Entry {
                path: std::mem::take(&mut paths[i]),
                mode: modes[i],
                mtime_s: mt[i],
                mtime_ns: mns[i],
                kind,
            });
        }
        r.finish()?;
        // Nothing may live "inside" a file or a symlink (e.g. link `a -> /etc` plus file `a/x`).
        let mut non_dirs = std::collections::HashSet::new();
        for e in &entries {
            let mut p = e.path.as_str();
            while let Some(k) = p.rfind('/') {
                p = &p[..k];
                ensure!(
                    !non_dirs.contains(p),
                    "entry {:?} is inside a non-directory entry",
                    e.path
                );
            }
            if !matches!(e.kind, Kind::Dir) {
                non_dirs.insert(e.path.as_str());
            }
        }
        Ok(Catalog {
            hash_len,
            created,
            creator,
            chunk_lens,
            entries,
        })
    }

    pub fn chunk_map(&self) -> ChunkMap {
        let n: usize = self.chunk_lens.iter().map(|v| v.len()).sum();
        let mut m = ChunkMap {
            block: Vec::with_capacity(n),
            offset: Vec::with_capacity(n),
            len: Vec::with_capacity(n),
        };
        for (bi, lens) in self.chunk_lens.iter().enumerate() {
            let mut off = 0u32;
            for &l in lens {
                m.block.push(bi as u32);
                m.offset.push(off);
                m.len.push(l);
                off += l;
            }
        }
        m
    }
}

impl ChunkMap {
    pub fn file_size(&self, chunks: &[u32]) -> u64 {
        chunks.iter().map(|&c| self.len[c as usize] as u64).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(raw: &[u32]) -> BlockTable {
        BlockTable {
            blocks: raw
                .iter()
                .map(|&r| BlockEntry {
                    raw_len: r,
                    stored_len: r,
                    hash: [7; 32],
                })
                .collect(),
        }
    }

    fn sample() -> (BlockTable, Catalog) {
        let t = table(&[30, 10]);
        let c = Catalog {
            hash_len: 16,
            created: 1_790_000_000,
            creator: "test".into(),
            chunk_lens: vec![vec![10, 20], vec![10]],
            entries: vec![
                Entry {
                    path: "a".into(),
                    mode: 0o755,
                    mtime_s: 5,
                    mtime_ns: 1,
                    kind: Kind::Dir,
                },
                Entry {
                    path: "a/x.txt".into(),
                    mode: 0o644,
                    mtime_s: -3,
                    mtime_ns: 999_999_999,
                    kind: Kind::File {
                        transform: 0,
                        chunks: vec![0, 1, 0],
                        hash: vec![1; 16],
                    },
                },
                Entry {
                    path: "a/y".into(),
                    mode: 0o777,
                    mtime_s: 9,
                    mtime_ns: 0,
                    kind: Kind::Symlink {
                        target: "x.txt".into(),
                    },
                },
                Entry {
                    path: "a/z".into(),
                    mode: 0o600,
                    mtime_s: 9,
                    mtime_ns: 0,
                    kind: Kind::File {
                        transform: 2,
                        chunks: vec![2],
                        hash: vec![2; 16],
                    },
                },
            ],
        };
        (t, c)
    }

    #[test]
    fn catalog_roundtrip() {
        let (t, c) = sample();
        let t2 = BlockTable::decode(&t.encode()).unwrap();
        assert_eq!(t2.blocks.len(), 2);
        let d = Catalog::decode(&c.encode(), &t2).unwrap();
        assert_eq!(d.entries.len(), 4);
        assert_eq!(d.entries[1].mtime_s, -3);
        let cm = d.chunk_map();
        match &d.entries[1].kind {
            Kind::File { chunks, .. } => assert_eq!(cm.file_size(chunks), 40),
            _ => panic!(),
        }
        match &d.entries[2].kind {
            Kind::Symlink { target } => assert_eq!(target, "x.txt"),
            _ => panic!(),
        }
    }

    #[test]
    fn rejects_bad_catalogs() {
        let (t, c) = sample();
        for bad in [
            "../evil",
            "/etc/passwd",
            "a/../../b",
            "C:/x",
            "a//b",
            "a\\..\\b",
            "./a",
        ] {
            let mut c2 = c.clone();
            c2.entries[0].path = bad.into();
            c2.entries.sort_by(|a, b| a.path.cmp(&b.path));
            assert!(Catalog::decode(&c2.encode(), &t).is_err(), "accepted {bad}");
        }
        // something inside a symlink entry
        let mut c6 = c.clone();
        c6.entries[3].path = "a/y/z".into();
        assert!(Catalog::decode(&c6.encode(), &t).is_err());
        // duplicate path
        let mut c3 = c.clone();
        c3.entries[2].path = "a/x.txt".into();
        assert!(Catalog::decode(&c3.encode(), &t).is_err());
        // chunk sizes not matching block size
        let mut c4 = c.clone();
        c4.chunk_lens[0] = vec![10, 21];
        assert!(Catalog::decode(&c4.encode(), &t).is_err());
        // chunk reference out of range
        let mut c5 = c.clone();
        if let Kind::File { chunks, .. } = &mut c5.entries[3].kind {
            chunks[0] = 99;
        }
        assert!(Catalog::decode(&c5.encode(), &t).is_err());
        // truncated / trailing data
        let enc = c.encode();
        assert!(Catalog::decode(&enc[..enc.len() - 1], &t).is_err());
        let mut longer = enc.clone();
        longer.push(0);
        assert!(Catalog::decode(&longer, &t).is_err());
    }

    #[test]
    fn varints() {
        for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let mut o = Vec::new();
            put_uv(&mut o, v);
            assert_eq!(ByteReader::new(&o).uv().unwrap(), v);
        }
        for v in [0i64, -1, 1, i64::MIN, i64::MAX, -123456789] {
            let mut o = Vec::new();
            put_sv(&mut o, v);
            assert_eq!(ByteReader::new(&o).sv().unwrap(), v);
        }
        assert!(ByteReader::new(&[0xFF; 11]).uv().is_err());
    }

    #[test]
    fn header_trailer() {
        let h = Header::build(HF_SIGNED, [3; 16], None);
        let p = Header::parse(h.bytes.clone()).unwrap();
        assert!(p.signed() && !p.encrypted());
        let mut bad = h.bytes.clone();
        bad[10] |= 0x80; // unknown flag
        assert!(Header::parse(bad).is_err());
        let mut v2 = h.bytes.clone();
        v2[8] = 2; // future major version
        assert!(Header::parse(v2).is_err());
        let tr = Trailer {
            index_offset: 100,
            archive_len: 300,
            root_hash: [9; 32],
            flags: 1,
        };
        let e = tr.encode();
        assert_eq!(Trailer::decode(&e).unwrap().index_offset, 100);
        let mut e2 = e;
        e2[3] ^= 1;
        assert!(Trailer::decode(&e2).is_err());
    }
}
