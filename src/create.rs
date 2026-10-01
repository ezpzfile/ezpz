//! Archive writer.

use crate::classify::{self, Class};
use crate::brain::Profile;
use crate::codec::{self, Plan};
use crate::format::*;
use crate::index::*;
use crate::{crypto, filter};
use anyhow::{Context, Result, anyhow, bail};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const CDC_MIN: u32 = 16 * 1024;
pub const CDC_AVG: u32 = 64 * 1024;
pub const CDC_MAX: u32 = 256 * 1024;
const WHOLE_READ_LIMIT: u64 = 64 << 20;
pub const DEFAULT_LEVEL: u8 = 7;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CodecChoice {
    Auto,
    Zstd,
    Lzma2,
    Brain,
    BrainFast,
    Store,
}

pub struct CreateOptions {
    pub level: u8,
    pub codec: CodecChoice,
    /// Fixed brain-fast model mask (testing); None lets the encoder choose per block.
    pub brain_mask: Option<u16>,
    pub block_size: Option<usize>,
    pub threads: usize,
    pub password: Option<Vec<u8>>,
    pub signing_key: Option<ed25519_dalek::SigningKey>,
    pub hash_len: u8,
    pub dedup: bool,
    pub filters: bool,
    pub verbose: bool,
}

#[derive(Default, Debug)]
pub struct Stats {
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub input_bytes: u64,
    pub unique_bytes: u64,
    pub dedup_bytes: u64,
    pub archive_bytes: u64,
    pub blocks: u64,
    pub codec_blocks: [u64; 6],
    pub seconds: f64,
}

pub const MAX_LEVEL: u8 = 11;

/// (plan, block size) for a level.
/// 10 = `--max`: fast brain codec with 16 MiB blocks, so several cores work at once.
/// 11 = smallest: full brain codec with 64 MiB blocks (slowest).
pub fn level_params(level: u8, choice: CodecChoice, brain_mask: Option<u16>) -> (Plan, usize) {
    let fast = match brain_mask {
        Some(m) => Profile::FastMask(m),
        None => Profile::Fast,
    };
    let mib = |n: usize| n << 20;
    let (zl, lz, bs) = match level {
        1 => (1, 1, mib(4)),
        2 => (3, 2, mib(4)),
        3 => (6, 4, mib(8)),
        4 => (9, 5, mib(8)),
        5 => (12, 6, mib(16)),
        6 => (16, 7, mib(16)),
        7 => (19, 8, mib(32)),
        8 => (22, 9, mib(64)),
        9 => (22, 9 | liblzma::stream::PRESET_EXTREME, mib(64)),
        10 => (22, 9 | liblzma::stream::PRESET_EXTREME, mib(16)),
        _ => (22, 9 | liblzma::stream::PRESET_EXTREME, mib(64)),
    };
    let plan = match choice {
        CodecChoice::Store => Plan::Store,
        CodecChoice::Zstd => Plan::Zstd(zl),
        CodecChoice::Lzma2 => Plan::Lzma2(lz),
        CodecChoice::Brain => Plan::Brain(Profile::Full),
        CodecChoice::BrainFast => Plan::Brain(fast),
        CodecChoice::Auto => match level {
            0..=8 => Plan::Zstd(zl),
            9 => Plan::Auto { zstd: zl, lzma: lz },
            10 => Plan::Brain(fast),
            _ => Plan::Brain(Profile::Full),
        },
    };
    (plan, bs)
}

struct Item {
    abs: PathBuf,
    rel: String,
    kind: ItemKind,
    mode: u32,
    mtime_s: i64,
    mtime_ns: u32,
    size: u64,
    class: Class,
    transform: u8,
}

enum ItemKind {
    File,
    Dir,
    Symlink(String),
}

fn mtime_of(m: &fs::Metadata) -> (i64, u32) {
    match m.modified() {
        Ok(t) => match t.duration_since(UNIX_EPOCH) {
            Ok(d) => (d.as_secs() as i64, d.subsec_nanos()),
            Err(e) => {
                let d = e.duration();
                if d.subsec_nanos() == 0 {
                    (-(d.as_secs() as i64), 0)
                } else {
                    (-(d.as_secs() as i64) - 1, 1_000_000_000 - d.subsec_nanos())
                }
            }
        },
        Err(_) => (0, 0),
    }
}

#[cfg(unix)]
fn mode_of(m: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    m.permissions().mode() & 0o7777
}
#[cfg(not(unix))]
fn mode_of(m: &fs::Metadata) -> u32 {
    if m.is_dir() {
        0o755
    } else if m.permissions().readonly() {
        0o444
    } else {
        0o644
    }
}

fn collect(inputs: &[PathBuf], skip: &HashSet<PathBuf>, verbose: bool) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for input in inputs {
        fs::symlink_metadata(input).with_context(|| format!("cannot read {}", input.display()))?;
        let root_name = match input.file_name().and_then(|n| n.to_str()) {
            Some(n) if n != "." && n != ".." => n.to_string(),
            _ => fs::canonicalize(input)?
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| anyhow!("cannot archive {}", input.display()))?
                .to_string(),
        };
        for e in walkdir::WalkDir::new(input)
            .follow_links(false)
            .sort_by_file_name()
        {
            let e = e?;
            let p = e.path();
            let relp = p.strip_prefix(input).unwrap_or(p);
            let mut rel = root_name.clone();
            let mut ok = true;
            for c in relp.components() {
                match c.as_os_str().to_str() {
                    Some(s) => {
                        rel.push('/');
                        rel.push_str(s);
                    }
                    None => ok = false,
                }
            }
            if !ok || validate_path(&rel).is_err() {
                eprintln!("warning: skipping unsupported file name {}", p.display());
                continue;
            }
            let ft = e.file_type();
            if ft.is_file() {
                if let Ok(c) = fs::canonicalize(p) {
                    if skip.contains(&c) {
                        continue;
                    }
                }
            }
            if !seen.insert(rel.clone()) {
                bail!("duplicate path in archive: {rel}");
            }
            let m = e.metadata()?;
            let (mtime_s, mtime_ns) = mtime_of(&m);
            let kind = if ft.is_dir() {
                ItemKind::Dir
            } else if ft.is_symlink() {
                let t = fs::read_link(p)?;
                match t.to_str() {
                    Some(s) if !s.is_empty() => ItemKind::Symlink(s.to_string()),
                    _ => {
                        eprintln!(
                            "warning: skipping symlink with unsupported target {}",
                            p.display()
                        );
                        continue;
                    }
                }
            } else if ft.is_file() {
                ItemKind::File
            } else {
                if verbose {
                    eprintln!("warning: skipping special file {}", p.display());
                }
                continue;
            };
            items.push(Item {
                abs: p.to_path_buf(),
                rel,
                kind,
                mode: mode_of(&m),
                mtime_s,
                mtime_ns,
                size: if ft.is_file() { m.len() } else { 0 },
                class: Class::Compressible,
                transform: 0,
            });
        }
    }
    Ok(items)
}

struct RawBlock {
    id: u32,
    data: Vec<u8>,
    lens: Vec<u32>,
    plan: Plan,
}

struct Producer {
    block_size: usize,
    dedup: bool,
    seen: HashMap<[u8; 32], u32>,
    cur: Vec<u8>,
    lens: Vec<u32>,
    cur_store: bool,
    next_chunk: u32,
    next_block: u32,
    plan: Plan,
    tx: mpsc::SyncSender<RawBlock>,
    unique_bytes: u64,
    dedup_bytes: u64,
}

impl Producer {
    fn add_chunk(&mut self, c: &[u8]) -> Result<u32> {
        let h = if self.dedup {
            Some(*blake3::hash(c).as_bytes())
        } else {
            None
        };
        if let Some(h) = &h {
            if let Some(&id) = self.seen.get(h) {
                self.dedup_bytes += c.len() as u64;
                return Ok(id);
            }
        }
        if !self.cur.is_empty() && self.cur.len() + c.len() > self.block_size {
            self.flush()?;
        }
        let id = self.next_chunk;
        self.next_chunk = self
            .next_chunk
            .checked_add(1)
            .ok_or_else(|| anyhow!("too many chunks"))?;
        self.cur.extend_from_slice(c);
        self.lens.push(c.len() as u32);
        self.unique_bytes += c.len() as u64;
        if let Some(h) = h {
            self.seen.insert(h, id);
        }
        Ok(id)
    }
    fn set_group(&mut self, store: bool) -> Result<()> {
        if store != self.cur_store {
            self.flush()?;
            self.cur_store = store;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        if self.cur.is_empty() {
            return Ok(());
        }
        let data = std::mem::replace(
            &mut self.cur,
            Vec::with_capacity(self.block_size + CDC_MAX as usize),
        );
        let lens = std::mem::take(&mut self.lens);
        let plan = if self.cur_store {
            Plan::Store
        } else {
            self.plan
        };
        self.tx
            .send(RawBlock {
                id: self.next_block,
                data,
                lens,
                plan,
            })
            .map_err(|_| anyhow!("compression worker stopped"))?;
        self.next_block += 1;
        Ok(())
    }
}

/// Counts the bytes written through it (zstd trial output).
struct ByteCount(u64);

impl Write for ByteCount {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0 += b.len() as u64;
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// x86-64 programs get transform 4 from the classifier. It wins clearly on large programs and on
/// several builds of the same program, but on a set of unrelated small libraries the in-place
/// transform 3 can come out about 1.5% smaller. So all of them are compressed in storage order
/// with fast zstd (long window), once each way, and the smaller result decides for the whole
/// set (SPEC.md §15.1). The other files and the format are not affected.
fn choose_x86_64_transform(items: &mut [Item], verbose: bool) {
    let mut idx: Vec<usize> = (0..items.len())
        .filter(|&i| matches!(items[i].kind, ItemKind::File) && items[i].transform == filter::XF_X86_64_SPLIT)
        .collect();
    if idx.is_empty() {
        return;
    }
    // Same order as in write_body (class and transform are equal here).
    idx.sort_by(|&a, &b| {
        let k = |it: &Item| {
            (
                it.size,
                classify::extension(&it.rel),
                it.rel.rsplit('/').next().unwrap_or("").to_string(),
                it.rel.clone(),
            )
        };
        k(&items[a]).cmp(&k(&items[b]))
    });
    let total: u64 = idx.iter().map(|&i| items[i].size).sum();
    let trial = || -> Result<(u64, u64)> {
        let enc = || -> Result<zstd::stream::write::Encoder<'static, ByteCount>> {
            let mut e = zstd::stream::write::Encoder::new(ByteCount(0), 1)?;
            e.long_distance_matching(true)?;
            e.window_log(26)?;
            e.set_pledged_src_size(Some(total))?;
            Ok(e)
        };
        let (mut a, mut b) = (enc()?, enc()?);
        for &i in &idx {
            let mut t4 = fs::read(&items[i].abs)?;
            let mut t3 = t4.clone();
            let (ra, rb) = rayon::join(
                || {
                    filter::encode(filter::XF_X86_64, &mut t3);
                    a.write_all(&t3)
                },
                || {
                    filter::encode(filter::XF_X86_64_SPLIT, &mut t4);
                    b.write_all(&t4)
                },
            );
            ra?;
            rb?;
        }
        Ok((a.finish()?.0, b.finish()?.0))
    };
    // A file that cannot be read now fails later with a proper error; keep transform 4.
    if let Ok((n3, n4)) = trial() {
        if verbose {
            eprintln!("x86-64 trial over {} files: transform 3 -> {n3}, transform 4 -> {n4} bytes", idx.len());
        }
        if n3 < n4 {
            for &i in &idx {
                items[i].transform = filter::XF_X86_64;
            }
        }
    }
}

/// Returns the chunk ids, the file hash, and the transform actually used.
fn process_file(p: &mut Producer, it: &Item, hash_len: usize) -> Result<(Vec<u32>, Vec<u8>, u8)> {
    let mut f = File::open(&it.abs).with_context(|| format!("cannot open {}", it.abs.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut ids = Vec::new();
    let mut used = it.transform;
    if it.transform != filter::XF_NONE || it.size <= WHOLE_READ_LIMIT {
        let mut buf = Vec::with_capacity(it.size as usize);
        f.read_to_end(&mut buf)?;
        hasher.update(&buf);
        if used == filter::XF_X86_64_SPLIT || used == filter::XF_X86_64 {
            // Safety net: decoding must give back the exact input. It always should,
            // but if it ever did not, fall back to transform 3 and then to the plain
            // x86 transform, which is reversible for any input.
            let before = blake3::hash(&buf);
            let mut chain = vec![used];
            if used == filter::XF_X86_64_SPLIT {
                chain.push(filter::XF_X86_64);
            }
            used = filter::XF_X86;
            for xf in chain {
                filter::encode(xf, &mut buf);
                let encoded = buf.clone();
                filter::decode(xf, &mut buf);
                if blake3::hash(&buf) == before {
                    buf = encoded;
                    used = xf;
                    break;
                }
                buf.clear();
                File::open(&it.abs)?.read_to_end(&mut buf)?;
            }
            if used == filter::XF_X86 {
                filter::encode(used, &mut buf);
            }
        } else {
            filter::encode(used, &mut buf);
        }
        if !buf.is_empty() {
            for c in fastcdc::v2020::FastCDC::new(&buf, CDC_MIN, CDC_AVG, CDC_MAX) {
                ids.push(p.add_chunk(&buf[c.offset..c.offset + c.length])?);
            }
        }
    } else {
        for c in fastcdc::v2020::StreamCDC::new(f, CDC_MIN, CDC_AVG, CDC_MAX) {
            let c = c.map_err(|e| anyhow!("reading {}: {e:?}", it.abs.display()))?;
            hasher.update(&c.data);
            ids.push(p.add_chunk(&c.data)?);
        }
    }
    Ok((ids, hasher.finalize().as_bytes()[..hash_len].to_vec(), used))
}

struct Sealed {
    frame: Vec<u8>,
    entry: BlockEntry,
    codec: u8,
}

fn seal_block(b: &RawBlock, key: Option<&[u8; 32]>, archive_id: &[u8; 16]) -> Result<Sealed> {
    let (codec, payload) = codec::compress(&b.data, b.plan)?;
    let stored_len = payload.len() + if key.is_some() { 16 } else { 0 };
    let fh = FrameHeader {
        magic: BLOCK_MAGIC,
        codec,
        flags: if key.is_some() { FF_ENCRYPTED } else { 0 },
        raw_len: b.data.len() as u32,
        stored_len: u32::try_from(stored_len)?,
    };
    let fhb = fh.encode();
    let payload = match key {
        Some(k) => crypto::seal(k, archive_id, b.id as u64, &fhb, &payload)?,
        None => payload,
    };
    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    frame.extend_from_slice(&fhb);
    frame.extend_from_slice(&payload);
    let hash = *blake3::hash(&frame).as_bytes();
    Ok(Sealed {
        frame,
        entry: BlockEntry {
            raw_len: fh.raw_len,
            stored_len: fh.stored_len,
            hash,
        },
        codec,
    })
}

fn make_frame(
    magic: [u8; 4],
    raw: &[u8],
    key: Option<&[u8; 32]>,
    archive_id: &[u8; 16],
    counter: u64,
) -> Result<Vec<u8>> {
    let (codec, payload) = codec::compress_index(raw)?;
    let stored_len = payload.len() + if key.is_some() { 16 } else { 0 };
    let fh = FrameHeader {
        magic,
        codec,
        flags: if key.is_some() { FF_ENCRYPTED } else { 0 },
        raw_len: u32::try_from(raw.len())?,
        stored_len: u32::try_from(stored_len)?,
    };
    let fhb = fh.encode();
    let payload = match key {
        Some(k) => crypto::seal(k, archive_id, counter, &fhb, &payload)?,
        None => payload,
    };
    let mut v = fhb.to_vec();
    v.extend_from_slice(&payload);
    Ok(v)
}

pub fn create(out: &Path, inputs: &[PathBuf], o: &CreateOptions) -> Result<Stats> {
    let t0 = Instant::now();
    let (plan, default_bs) = level_params(o.level, o.codec, o.brain_mask);
    let block_size = o
        .block_size
        .unwrap_or(default_bs)
        .clamp(1 << 20, MAX_BLOCK_RAW as usize - CDC_MAX as usize);

    // Output goes to a temp file first, renamed at the end.
    let out_dir = match out.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let out_name = out
        .file_name()
        .ok_or_else(|| anyhow!("bad output path"))?
        .to_string_lossy()
        .to_string();
    let tmp = out_dir.join(format!(".{out_name}.partial-{}", std::process::id()));
    let mut skip = HashSet::new();
    if let Ok(d) = fs::canonicalize(&out_dir) {
        skip.insert(d.join(&out_name));
        skip.insert(d.join(tmp.file_name().unwrap()));
    }

    let mut items = collect(inputs, &skip, o.verbose)?;
    ensure_nonempty(&items)?;

    // Classify files (reads a 64 KiB sample of each).
    items
        .par_iter_mut()
        .filter(|i| matches!(i.kind, ItemKind::File))
        .for_each(|it| {
            let mut sample = vec![0u8; 65536.min(it.size as usize)];
            let n = File::open(&it.abs)
                .and_then(|mut f| read_full(&mut f, &mut sample))
                .unwrap_or(0);
            sample.truncate(n);
            let (c, xf) = classify::classify(&it.abs, &it.rel, &sample, o.filters);
            it.class = c;
            it.transform = xf;
        });
    if o.level >= 4 {
        choose_x86_64_transform(&mut items, o.verbose);
    }

    // Header
    let mut archive_id = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut archive_id);
    let mut flags = 0u16;
    let mut enc = None;
    let mut key = None;
    if let Some(pw) = &o.password {
        let mut salt = [0u8; 16];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);
        let e = Encryption {
            cipher: CIPHER_XCHACHA20POLY1305,
            kdf: KDF_ARGON2ID,
            salt,
            m_kib: crypto::DEFAULT_M_KIB,
            t_cost: crypto::DEFAULT_T,
            p_lanes: crypto::DEFAULT_P,
        };
        key = Some(crypto::derive_key(pw, &e)?);
        enc = Some(e);
        flags |= HF_ENCRYPTED;
    }
    if o.signing_key.is_some() {
        flags |= HF_SIGNED;
    }
    let header = Header::build(flags, archive_id, enc);

    let file = File::create(&tmp).with_context(|| format!("cannot create {}", tmp.display()))?;
    let res = write_body(file, &header, &items, o, plan, block_size, key, archive_id);
    match res {
        Ok(mut st) => {
            fs::rename(&tmp, out)
                .with_context(|| format!("cannot move archive into place at {}", out.display()))?;
            st.seconds = t0.elapsed().as_secs_f64();
            Ok(st)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_body(
    file: File,
    header: &Header,
    items: &[Item],
    o: &CreateOptions,
    plan: Plan,
    block_size: usize,
    key: Option<[u8; 32]>,
    archive_id: [u8; 16],
) -> Result<Stats> {
    let mut st = Stats::default();
    let mut w = BufWriter::with_capacity(1 << 20, file);
    w.write_all(&header.bytes)?;

    // Compression worker: batches of blocks compressed in parallel, written in order.
    let threads = o.threads.max(1);
    let (tx, rx) = mpsc::sync_channel::<RawBlock>(threads * 2);
    let header_len = header.bytes.len() as u64;
    let worker = std::thread::spawn(move || -> Result<(BufWriter<File>, Vec<BlockEntry>, Vec<Vec<u32>>, [u64; 6], u64)> {
        let mut entries = Vec::new();
        let mut lens_all = Vec::new();
        let mut counts = [0u64; 6];
        let mut offset = header_len;
        let mut done = false;
        while !done {
            let mut batch = Vec::with_capacity(threads);
            while batch.len() < threads {
                match rx.recv() {
                    Ok(b) => batch.push(b),
                    Err(_) => {
                        done = true;
                        break;
                    }
                }
            }
            if batch.is_empty() {
                break;
            }
            let sealed: Vec<Result<Sealed>> = batch.par_iter().map(|b| seal_block(b, key.as_ref(), &archive_id)).collect();
            for (s, b) in sealed.into_iter().zip(batch.into_iter()) {
                let s = s?;
                w.write_all(&s.frame)?;
                offset += s.frame.len() as u64;
                counts[s.codec.min(5) as usize] += 1;
                entries.push(s.entry);
                lens_all.push(b.lens);
            }
        }
        Ok((w, entries, lens_all, counts, offset))
    });

    let mut prod = Producer {
        block_size,
        dedup: o.dedup,
        seen: HashMap::new(),
        cur: Vec::with_capacity(block_size + CDC_MAX as usize),
        lens: Vec::new(),
        cur_store: false,
        next_chunk: 0,
        next_block: 0,
        plan,
        tx,
        unique_bytes: 0,
        dedup_bytes: 0,
    };

    // Storage order: similar files next to each other.
    let mut order: Vec<usize> = (0..items.len())
        .filter(|&i| matches!(items[i].kind, ItemKind::File))
        .collect();
    // Machine code: by size, so builds of the same program (similar size) sit together and one
    // huge binary doesn't push related files apart. Everything else: by extension, then file name,
    // so e.g. the same file from different versions of a project ends up side by side.
    let key_of = |it: &Item| {
        let size_key = if it.class == Class::Exec { it.size } else { 0 };
        (
            it.class,
            it.transform,
            size_key,
            classify::extension(&it.rel),
            it.rel.rsplit('/').next().unwrap_or("").to_string(),
        )
    };
    order.sort_by(|&a, &b| {
        (key_of(&items[a]), &items[a].rel).cmp(&(key_of(&items[b]), &items[b].rel))
    });

    let hash_len = o.hash_len as usize;
    let mut file_data: Vec<Option<(Vec<u32>, Vec<u8>, u8)>> = vec![None; items.len()];
    let mut produce_err = None;
    for &i in &order {
        let it = &items[i];
        let r = prod
            .set_group(it.class == Class::Incompressible)
            .and_then(|_| process_file(&mut prod, it, hash_len));
        match r {
            Ok(v) => {
                st.input_bytes += it.size;
                file_data[i] = Some(v);
            }
            Err(e) => {
                produce_err = Some(e);
                break;
            }
        }
    }
    if produce_err.is_none() {
        if let Err(e) = prod.flush() {
            produce_err = Some(e);
        }
    }
    st.unique_bytes = prod.unique_bytes;
    st.dedup_bytes = prod.dedup_bytes;
    drop(prod);
    let joined = worker
        .join()
        .map_err(|_| anyhow!("compression worker panicked"))?;
    if let Some(e) = produce_err {
        return Err(e);
    }
    let (mut w, block_entries, chunk_lens, counts, mut offset) = joined?;
    st.codec_blocks = counts;
    st.blocks = block_entries.len() as u64;

    // Catalog entries, sorted by path.
    let mut entries: Vec<Entry> = Vec::with_capacity(items.len());
    for (i, it) in items.iter().enumerate() {
        let kind = match &it.kind {
            ItemKind::Dir => {
                st.dirs += 1;
                Kind::Dir
            }
            ItemKind::Symlink(t) => {
                st.symlinks += 1;
                Kind::Symlink { target: t.clone() }
            }
            ItemKind::File => {
                st.files += 1;
                let (chunks, hash, transform) = file_data[i].take().unwrap();
                Kind::File {
                    transform,
                    chunks,
                    hash,
                }
            }
        };
        entries.push(Entry {
            path: it.rel.clone(),
            mode: it.mode,
            mtime_s: it.mtime_s,
            mtime_ns: it.mtime_ns,
            kind,
        });
    }
    entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));

    let table = BlockTable {
        blocks: block_entries,
    };
    let catalog = Catalog {
        hash_len: o.hash_len,
        created: now_unix(),
        creator: format!("ezpz-rs {}", env!("CARGO_PKG_VERSION")),
        chunk_lens,
        entries,
    };

    let index_offset = offset;
    let tframe = make_frame(TABLE_MAGIC, &table.encode(), None, &archive_id, 0)?;
    let cframe = make_frame(
        CATALOG_MAGIC,
        &catalog.encode(),
        key.as_ref(),
        &archive_id,
        crypto::CATALOG_COUNTER,
    )?;
    let mut region = tframe.clone();
    region.extend_from_slice(&cframe);
    let root = root_hash(&header.bytes, &region);
    w.write_all(&tframe)?;
    w.write_all(&cframe)?;
    offset += (tframe.len() + cframe.len()) as u64;

    let mut tflags = 0u32;
    if let Some(sk) = &o.signing_key {
        use ed25519_dalek::Signer;
        let sig = sk.sign(&signed_message(&root));
        let sec = SigSection {
            algo: SIG_ED25519,
            public_key: sk.verifying_key().to_bytes(),
            signature: sig.to_bytes(),
        };
        w.write_all(&sec.encode())?;
        offset += SIG_SECTION_LEN as u64;
        tflags |= TF_SIGNATURE;
    }
    let archive_len = offset + TRAILER_LEN as u64;
    let tr = Trailer {
        index_offset,
        archive_len,
        root_hash: root,
        flags: tflags,
    };
    w.write_all(&tr.encode())?;
    let f = w
        .into_inner()
        .map_err(|e| anyhow!("write failed: {}", e.error()))?;
    f.sync_all()?;
    st.archive_bytes = archive_len;
    Ok(st)
}

fn ensure_nonempty(items: &[Item]) -> Result<()> {
    if items.is_empty() {
        bail!("nothing to archive");
    }
    Ok(())
}

fn read_full(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let k = f.read(&mut buf[n..])?;
        if k == 0 {
            break;
        }
        n += k;
    }
    Ok(n)
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
