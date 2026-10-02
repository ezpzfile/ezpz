//! Archive reader: open, verify, random-access file reads, extraction.
//! An archive can be read from a file or from bytes in memory (the WebAssembly build).

use crate::format::*;
use crate::index::*;
use crate::{codec, crypto, filter};
use anyhow::{Context, Result, anyhow, bail, ensure};
use rayon::prelude::*;
use std::collections::{HashMap, VecDeque};
#[cfg(not(target_arch = "wasm32"))]
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::Arc;

/// Where the archive bytes come from.
pub enum Source {
    #[cfg(not(target_arch = "wasm32"))]
    File(File),
    Mem(Vec<u8>),
}

impl Source {
    fn len(&self) -> io::Result<u64> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Source::File(f) => Ok(f.metadata()?.len()),
            Source::Mem(v) => Ok(v.len() as u64),
        }
    }

    fn read_at(&self, buf: &mut [u8], off: u64) -> io::Result<()> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Source::File(f) => read_at(f, buf, off),
            Source::Mem(v) => {
                let start = usize::try_from(off).map_err(|_| io::Error::from(io::ErrorKind::UnexpectedEof))?;
                let end = start
                    .checked_add(buf.len())
                    .filter(|&e| e <= v.len())
                    .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
                buf.copy_from_slice(&v[start..end]);
                Ok(())
            }
        }
    }
}

pub struct Archive {
    src: Source,
    pub len: u64,
    pub header: Header,
    #[allow(dead_code)]
    pub trailer: Trailer,
    pub table: BlockTable,
    pub block_offsets: Vec<u64>,
    pub signature: Option<SigSection>,
    pub digest: [u8; 32],
    pub catalog_frame: FrameHeader,
    pub table_frame: FrameHeader,
    catalog_bytes: Vec<u8>,
    catalog_offset: u64,
    key: Option<[u8; 32]>,
    pub catalog: Option<Catalog>,
    pub chunks: Option<ChunkMap>,
}

#[cfg(not(target_arch = "wasm32"))]
fn read_at(f: &File, buf: &mut [u8], off: u64) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(buf, off)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut done = 0;
        while done < buf.len() {
            let n = f.seek_read(&mut buf[done..], off + done as u64)?;
            if n == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            }
            done += n;
        }
        Ok(())
    }
}

impl Archive {
    /// Opens and authenticates the archive structure. The catalog is decoded only if the archive is
    /// unencrypted or `password` yields a key. With `need_catalog=false` an encrypted archive can be
    /// structurally verified without the password.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(
        path: &Path,
        password: &mut dyn FnMut() -> Result<Vec<u8>>,
        need_catalog: bool,
    ) -> Result<Archive> {
        let f = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        Self::open_source(Source::File(f), password, need_catalog)
    }

    /// Opens an archive held in memory. Without a password, an encrypted archive is opened
    /// structurally only (`has_catalog()` is false): block hashes and the signature can still
    /// be checked, but nothing can be listed or extracted.
    pub fn from_bytes(data: Vec<u8>, password: Option<&[u8]>) -> Result<Archive> {
        let need = password.is_some();
        let pw = password.map(|p| p.to_vec());
        Self::open_source(
            Source::Mem(data),
            &mut || pw.clone().ok_or_else(|| anyhow!("password required")),
            need,
        )
    }

    /// Opens and authenticates the archive structure from any source (see `open`).
    pub fn open_source(
        src: Source,
        password: &mut dyn FnMut() -> Result<Vec<u8>>,
        need_catalog: bool,
    ) -> Result<Archive> {
        let f = &src;
        let len = f.len()?;
        ensure!(
            len >= (HEADER_FIXED_LEN + TRAILER_LEN) as u64,
            "file too small to be an .ezpz archive"
        );

        let mut fixed = [0u8; HEADER_FIXED_LEN];
        f.read_at(&mut fixed, 0)?;
        let ext_len = Header::ext_len(&fixed)? as usize;
        let mut hb = vec![0u8; HEADER_FIXED_LEN + ext_len];
        f.read_at(&mut hb, 0)?;
        let header = Header::parse(hb)?;

        let mut tb = [0u8; TRAILER_LEN];
        f.read_at(&mut tb, len - TRAILER_LEN as u64)?;
        let trailer = Trailer::decode(&tb)?;
        ensure!(
            trailer.archive_len == len,
            "archive length mismatch: expected {} bytes, file has {} (truncated or extra data)",
            trailer.archive_len,
            len
        );
        let signed = trailer.flags & TF_SIGNATURE != 0;
        ensure!(
            signed == header.signed(),
            "signature flags in header and trailer disagree"
        );
        let index_end = len - TRAILER_LEN as u64 - if signed { SIG_SECTION_LEN as u64 } else { 0 };
        let hdr_len = header.bytes.len() as u64;
        ensure!(
            trailer.index_offset >= hdr_len
                && trailer.index_offset + 2 * FRAME_HEADER_LEN as u64 <= index_end,
            "index offset out of range"
        );

        // Index region = block table frame + catalog frame. root_hash covers header + index region.
        let mut region = vec![0u8; (index_end - trailer.index_offset) as usize];
        f.read_at(&mut region, trailer.index_offset)?;
        let digest = root_hash(&header.bytes, &region);
        ensure!(
            digest == trailer.root_hash,
            "archive checksum mismatch (header or index damaged or tampered with)"
        );

        let signature = if signed {
            let mut sb = [0u8; SIG_SECTION_LEN];
            f.read_at(&mut sb, index_end)?;
            let s = SigSection::decode(&sb)?;
            let vk = ed25519_dalek::VerifyingKey::from_bytes(&s.public_key)
                .map_err(|_| anyhow!("bad public key"))?;
            let sig = ed25519_dalek::Signature::from_bytes(&s.signature);
            vk.verify_strict(&signed_message(&digest), &sig)
                .map_err(|_| anyhow!("SIGNATURE INVALID - archive was modified after signing"))?;
            Some(s)
        } else {
            None
        };

        let table_frame = FrameHeader::decode(&region, TABLE_MAGIC)?;
        ensure!(
            !table_frame.encrypted(),
            "block table must not be encrypted"
        );
        ensure!(
            table_frame.codec <= CODEC_LZMA2,
            "index frames must use store, zstd or lzma2"
        );
        let t_end = FRAME_HEADER_LEN + table_frame.stored_len as usize;
        ensure!(
            t_end + FRAME_HEADER_LEN <= region.len(),
            "bad block table size"
        );
        ensure!(
            table_frame.raw_len <= MAX_INDEX_RAW,
            "block table too large"
        );
        let traw = codec::decompress(
            table_frame.codec,
            &region[FRAME_HEADER_LEN..t_end],
            table_frame.raw_len as usize,
        )?;
        let table = BlockTable::decode(&traw)?;
        let (block_offsets, blocks_end) = table.offsets(hdr_len);
        ensure!(
            blocks_end == trailer.index_offset,
            "block table does not match file layout"
        );

        let catalog_frame = FrameHeader::decode(&region[t_end..], CATALOG_MAGIC)?;
        ensure!(
            catalog_frame.encrypted() == header.encrypted(),
            "catalog encryption flag mismatch"
        );
        ensure!(catalog_frame.raw_len <= MAX_INDEX_RAW, "catalog too large");
        ensure!(
            catalog_frame.codec <= CODEC_LZMA2,
            "index frames must use store, zstd or lzma2"
        );
        ensure!(
            t_end + FRAME_HEADER_LEN + catalog_frame.stored_len as usize == region.len(),
            "bad catalog size"
        );
        let catalog_bytes = region[t_end..].to_vec();
        let catalog_offset = trailer.index_offset + t_end as u64;

        let mut a = Archive {
            src,
            len,
            header,
            trailer,
            table,
            block_offsets,
            signature,
            digest,
            catalog_frame,
            table_frame,
            catalog_bytes,
            catalog_offset,
            key: None,
            catalog: None,
            chunks: None,
        };
        if a.header.encrypted() {
            if need_catalog {
                let pw = password()?;
                let e = a.header.encryption.clone().unwrap();
                a.key = Some(crypto::derive_key(&pw, &e)?);
                a.load_catalog()?;
            }
        } else {
            a.load_catalog()?;
        }
        Ok(a)
    }

    fn load_catalog(&mut self) -> Result<()> {
        let fh = self.catalog_frame;
        let hdr: [u8; FRAME_HEADER_LEN] =
            self.catalog_bytes[..FRAME_HEADER_LEN].try_into().unwrap();
        let body = &self.catalog_bytes[FRAME_HEADER_LEN..];
        let plain = match &self.key {
            Some(k) => crypto::open(
                k,
                &self.header.archive_id,
                crypto::CATALOG_COUNTER,
                &hdr,
                body,
            )?,
            None => body.to_vec(),
        };
        let raw = codec::decompress(fh.codec, &plain, fh.raw_len as usize)?;
        let cat = Catalog::decode(&raw, &self.table)?;
        self.chunks = Some(cat.chunk_map());
        self.catalog = Some(cat);
        Ok(())
    }

    pub fn has_catalog(&self) -> bool {
        self.catalog.is_some()
    }
    pub fn catalog(&self) -> &Catalog {
        self.catalog.as_ref().expect("catalog not loaded")
    }
    pub fn chunk_map(&self) -> &ChunkMap {
        self.chunks.as_ref().expect("catalog not loaded")
    }
    pub fn catalog_offset(&self) -> u64 {
        self.catalog_offset
    }

    /// Reads one block, checks its BLAKE3 hash, and (if `decode`) decrypts + decompresses it.
    pub fn read_block(&self, id: u32, decode: bool) -> Result<Vec<u8>> {
        let b = &self.table.blocks[id as usize];
        let mut buf = vec![0u8; FRAME_HEADER_LEN + b.stored_len as usize];
        self.src.read_at(&mut buf, self.block_offsets[id as usize])?;
        ensure!(
            blake3::hash(&buf).as_bytes() == &b.hash,
            "block {id}: checksum mismatch (data damaged or tampered with)"
        );
        let fh = FrameHeader::decode(&buf, BLOCK_MAGIC)?;
        ensure!(
            fh.raw_len == b.raw_len && fh.stored_len == b.stored_len,
            "block {id}: header mismatch"
        );
        ensure!(
            fh.encrypted() == self.header.encrypted(),
            "block {id}: encryption flag mismatch"
        );
        if !decode {
            return Ok(Vec::new());
        }
        let payload = if fh.encrypted() {
            let k = self
                .key
                .as_ref()
                .ok_or_else(|| anyhow!("password required"))?;
            crypto::open(
                k,
                &self.header.archive_id,
                id as u64,
                &buf[..FRAME_HEADER_LEN],
                &buf[FRAME_HEADER_LEN..],
            )?
        } else {
            buf.drain(..FRAME_HEADER_LEN);
            buf
        };
        codec::decompress(fh.codec, &payload, fh.raw_len as usize)
            .with_context(|| format!("block {id}"))
    }

    /// Codec of one block, from its frame header (not hash-checked; for display).
    pub fn block_codec(&self, id: u32) -> Result<u8> {
        let mut fh = [0u8; FRAME_HEADER_LEN];
        self.src.read_at(&mut fh, self.block_offsets[id as usize])?;
        Ok(FrameHeader::decode(&fh, BLOCK_MAGIC)?.codec)
    }

    pub fn find(&self, path: &str) -> Option<&Entry> {
        let es = &self.catalog().entries;
        es.binary_search_by(|e| e.path.as_bytes().cmp(path.as_bytes()))
            .ok()
            .map(|i| &es[i])
    }

    /// Whole content of one file, hash checked. For many files, keep a `BlockCache` (or another
    /// `BlockSource`) so that blocks shared between files are decoded once.
    pub fn read_file(&self, e: &Entry) -> Result<Vec<u8>> {
        let mut cache = BlockCache::new(self, access_sequence(self, &[e]), 1);
        let mut out = Vec::new();
        write_content(self, &mut cache, e, &mut out)?;
        Ok(out)
    }
}

/// Gives decoded blocks to `write_content`.
pub trait BlockSource {
    fn block(&mut self, id: u32) -> Result<Arc<Vec<u8>>>;
}

impl BlockSource for BlockCache<'_> {
    fn block(&mut self, id: u32) -> Result<Arc<Vec<u8>>> {
        self.get(id)
    }
}

/// Decompressed-block cache with parallel read-ahead along a known access sequence.
pub struct BlockCache<'a> {
    ar: &'a Archive,
    cache: HashMap<u32, Arc<Vec<u8>>>,
    lru: VecDeque<u32>,
    cap: usize,
    seq: Vec<u32>,
    pos: usize,
    par: usize,
}

impl<'a> BlockCache<'a> {
    pub fn new(ar: &'a Archive, seq: Vec<u32>, par: usize) -> Self {
        let par = par.max(1);
        BlockCache {
            ar,
            cache: HashMap::new(),
            lru: VecDeque::new(),
            cap: par * 2 + 2,
            seq,
            pos: 0,
            par,
        }
    }

    pub fn get(&mut self, b: u32) -> Result<Arc<Vec<u8>>> {
        while self.pos < self.seq.len() && self.seq[self.pos] != b {
            self.pos += 1;
        }
        if let Some(v) = self.cache.get(&b) {
            let v = v.clone();
            self.lru.retain(|&x| x != b);
            self.lru.push_back(b);
            return Ok(v);
        }
        let mut want = vec![b];
        let mut i = self.pos;
        while want.len() < self.par && i < self.seq.len() {
            let x = self.seq[i];
            if !self.cache.contains_key(&x) && !want.contains(&x) {
                want.push(x);
            }
            i += 1;
        }
        let ar = self.ar;
        let got: Vec<Result<Vec<u8>>> = want.par_iter().map(|&x| ar.read_block(x, true)).collect();
        let mut first = None;
        for (x, r) in want.iter().zip(got) {
            let v = Arc::new(r?);
            if *x == b {
                first = Some(v);
            } else {
                self.insert(*x, v);
            }
        }
        let v = first.unwrap();
        self.insert(b, v.clone());
        Ok(v)
    }

    fn insert(&mut self, b: u32, v: Arc<Vec<u8>>) {
        self.cache.insert(b, v);
        self.lru.retain(|&x| x != b);
        self.lru.push_back(b);
        while self.lru.len() > self.cap {
            if let Some(old) = self.lru.pop_front() {
                self.cache.remove(&old);
            }
        }
    }
}

/// Block access sequence for reading `files` in order.
pub fn access_sequence(ar: &Archive, files: &[&Entry]) -> Vec<u32> {
    let cm = ar.chunk_map();
    let mut seq = Vec::new();
    for e in files {
        if let Kind::File { chunks, .. } = &e.kind {
            for &c in chunks {
                let b = cm.block[c as usize];
                if seq.last() != Some(&b) {
                    seq.push(b);
                }
            }
        }
    }
    seq
}

/// Streams one file's content to `out`, verifying its hash. Returns bytes written.
pub fn write_content(
    ar: &Archive,
    cache: &mut dyn BlockSource,
    e: &Entry,
    out: &mut dyn Write,
) -> Result<u64> {
    let Kind::File {
        transform,
        chunks,
        hash,
    } = &e.kind
    else {
        bail!("{} is not a file", e.path)
    };
    let cm = ar.chunk_map();
    let hl = ar.catalog().hash_len as usize;
    let mut hasher = blake3::Hasher::new();
    let mut n = 0u64;
    if *transform != filter::XF_NONE {
        let mut buf = Vec::with_capacity(cm.file_size(chunks) as usize);
        for &c in chunks {
            let (b, o, l) = (
                cm.block[c as usize],
                cm.offset[c as usize] as usize,
                cm.len[c as usize] as usize,
            );
            let blk = cache.block(b)?;
            buf.extend_from_slice(&blk[o..o + l]);
        }
        filter::decode(*transform, &mut buf);
        hasher.update(&buf);
        out.write_all(&buf)?;
        n = buf.len() as u64;
    } else {
        for &c in chunks {
            let (b, o, l) = (
                cm.block[c as usize],
                cm.offset[c as usize] as usize,
                cm.len[c as usize] as usize,
            );
            let blk = cache.block(b)?;
            let s = &blk[o..o + l];
            hasher.update(s);
            out.write_all(s)?;
            n += l as u64;
        }
    }
    if hl > 0 {
        ensure!(
            hasher.finalize().as_bytes()[..hl] == hash[..],
            "{}: content hash mismatch",
            e.path
        );
    }
    Ok(n)
}

#[cfg(not(target_arch = "wasm32"))]
pub struct ExtractOptions {
    pub dest: PathBuf,
    pub force: bool,
    pub threads: usize,
    pub unsafe_links: bool,
    pub verbose: bool,
}

#[cfg(not(target_arch = "wasm32"))]
fn selected<'a>(ar: &'a Archive, filters: &[String]) -> Result<Vec<&'a Entry>> {
    let es = &ar.catalog().entries;
    if filters.is_empty() {
        return Ok(es.iter().collect());
    }
    let mut out = Vec::new();
    for f in filters {
        let f = f.trim_end_matches('/');
        let pre = format!("{f}/");
        let before = out.len();
        out.extend(
            es.iter()
                .filter(|e| e.path == f || e.path.starts_with(&pre)),
        );
        ensure!(out.len() > before, "not found in archive: {f}");
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    Ok(out)
}

/// True if a symbolic link at `path` pointing to `target` stays inside the extraction folder.
pub fn link_is_safe(path: &str, target: &str) -> bool {
    if target.starts_with('/') || target.contains('\\') || Path::new(target).is_absolute() {
        return false;
    }
    let mut depth: i64 = path.matches('/').count() as i64; // directories above the link inside the root
    for c in Path::new(target).components() {
        match c {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            _ => return false,
        }
    }
    true
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(mode));
}
#[cfg(all(not(unix), not(target_arch = "wasm32")))]
fn set_mode(p: &Path, mode: u32) {
    if mode & 0o200 == 0 {
        if let Ok(m) = fs::metadata(p) {
            let mut perm = m.permissions();
            perm.set_readonly(true);
            let _ = fs::set_permissions(p, perm);
        }
    }
}

pub struct ExtractStats {
    pub files: u64,
    pub bytes: u64,
    pub skipped_links: u64,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn extract(ar: &Archive, filters: &[String], o: &ExtractOptions) -> Result<ExtractStats> {
    let sel = selected(ar, filters)?;
    let mut st = ExtractStats {
        files: 0,
        bytes: 0,
        skipped_links: 0,
    };
    fs::create_dir_all(&o.dest)?;
    let dest = &o.dest;

    for e in sel.iter().filter(|e| matches!(e.kind, Kind::Dir)) {
        fs::create_dir_all(dest.join(&e.path))?;
    }

    let mut files: Vec<&Entry> = sel
        .iter()
        .copied()
        .filter(|e| matches!(e.kind, Kind::File { .. }))
        .collect();
    files.sort_by_key(|e| match &e.kind {
        Kind::File { chunks, .. } => chunks.first().copied().unwrap_or(0),
        _ => 0,
    });
    let mut cache = BlockCache::new(ar, access_sequence(ar, &files), o.threads);
    for e in &files {
        let p = dest.join(&e.path);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Ok(m) = fs::symlink_metadata(&p) {
            if !o.force {
                bail!("{} already exists (use --force to overwrite)", p.display());
            }
            if m.is_dir() {
                bail!("{} is a directory", p.display());
            }
            fs::remove_file(&p)?;
        }
        let f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&p)
            .with_context(|| format!("cannot create {}", p.display()))?;
        let mut w = io::BufWriter::with_capacity(1 << 20, f);
        let n = write_content(ar, &mut cache, e, &mut w)?;
        w.flush()?;
        drop(w);
        set_mode(&p, e.mode & 0o777); // setuid/setgid/sticky are never restored
        let _ = filetime::set_file_mtime(
            &p,
            filetime::FileTime::from_unix_time(e.mtime_s, e.mtime_ns),
        );
        st.files += 1;
        st.bytes += n;
        if o.verbose {
            eprintln!("{}", e.path);
        }
    }

    // Symlinks last, so no file write can ever pass through a link from this archive.
    for e in sel.iter() {
        if let Kind::Symlink { target } = &e.kind {
            if !o.unsafe_links && !link_is_safe(&e.path, target) {
                eprintln!(
                    "warning: skipping symlink {} -> {} (points outside the extraction folder; use --unsafe-links)",
                    e.path, target
                );
                st.skipped_links += 1;
                continue;
            }
            let p = dest.join(&e.path);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent)?;
            }
            if fs::symlink_metadata(&p).is_ok() {
                if !o.force {
                    bail!("{} already exists (use --force to overwrite)", p.display());
                }
                fs::remove_file(&p)?;
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(target, &p)?;
                let _ = filetime::set_symlink_file_times(
                    &p,
                    filetime::FileTime::from_unix_time(e.mtime_s, e.mtime_ns),
                    filetime::FileTime::from_unix_time(e.mtime_s, e.mtime_ns),
                );
            }
            #[cfg(not(unix))]
            {
                eprintln!(
                    "warning: symlinks not supported on this platform: {}",
                    e.path
                );
            }
        }
    }

    // Directory metadata last (deepest first) so writing files doesn't bump mtimes.
    let mut dirs: Vec<&&Entry> = sel.iter().filter(|e| matches!(e.kind, Kind::Dir)).collect();
    dirs.sort_by_key(|e| std::cmp::Reverse(e.path.matches('/').count()));
    for e in dirs {
        let p = dest.join(&e.path);
        set_mode(&p, (e.mode & 0o777) | 0o700);
        let _ = filetime::set_file_mtime(
            &p,
            filetime::FileTime::from_unix_time(e.mtime_s, e.mtime_ns),
        );
    }
    Ok(st)
}

pub struct VerifyReport {
    pub blocks_ok: u64,
    pub blocks_bad: Vec<String>,
    pub files_ok: u64,
    pub files_bad: Vec<String>,
    pub content_checked: bool,
}

pub fn verify(ar: &Archive, threads: usize) -> VerifyReport {
    let decode = ar.has_catalog();
    let n = ar.table.blocks.len() as u32;
    let res: Vec<(u32, Result<Vec<u8>>)> = (0..n)
        .into_par_iter()
        .map(|i| (i, ar.read_block(i, false)))
        .collect();
    let mut rep = VerifyReport {
        blocks_ok: 0,
        blocks_bad: vec![],
        files_ok: 0,
        files_bad: vec![],
        content_checked: decode,
    };
    for (_, r) in res {
        match r {
            Ok(_) => rep.blocks_ok += 1,
            Err(e) => rep.blocks_bad.push(format!("{e:#}")),
        }
    }
    if decode && rep.blocks_bad.is_empty() {
        let mut files: Vec<&Entry> = ar
            .catalog()
            .entries
            .iter()
            .filter(|e| matches!(e.kind, Kind::File { .. }))
            .collect();
        files.sort_by_key(|e| match &e.kind {
            Kind::File { chunks, .. } => chunks.first().copied().unwrap_or(0),
            _ => 0,
        });
        let mut cache = BlockCache::new(ar, access_sequence(ar, &files), threads);
        for e in files {
            match write_content(ar, &mut cache, e, &mut io::sink()) {
                Ok(_) => rep.files_ok += 1,
                Err(err) => rep.files_bad.push(format!("{}: {err:#}", e.path)),
            }
        }
    }
    rep
}
