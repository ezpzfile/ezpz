//! WebAssembly build of ezpz: open, list, extract, verify, and create `.ezpz` archives in a
//! browser, without uploading anything. Build with `web/build.sh`; the JavaScript API is
//! described in `web/README.md`.
//!
//! What differs from the command-line tool: there are no threads (one block at a time), no file
//! system (input and output are byte arrays), and no LZMA2 encoder. LZMA2 blocks written by the
//! command-line tool at level 9 are still decoded, and level 9 here compresses with zstd alone.

use ezpz::archive::{Archive, BlockSource, write_content};
use ezpz::create::{self, CodecChoice, CreateOptions, MemEntry, MemKind};
use ezpz::format::codec_name;
use ezpz::index::{Entry, Kind};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::Write as _;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

/// Version of the ezpz library inside this build.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn js_err(e: anyhow::Error) -> JsError {
    JsError::new(&format!("{e:#}"))
}

/// A few decoded blocks kept between `read` calls, so files that share a block (most of
/// them, in a solid archive) do not decode it again.
struct BlockLru {
    map: HashMap<u32, Arc<Vec<u8>>>,
    order: VecDeque<u32>,
    cap: usize,
}

struct Cached<'a> {
    ar: &'a Archive,
    lru: &'a mut BlockLru,
}

impl BlockSource for Cached<'_> {
    fn block(&mut self, id: u32) -> anyhow::Result<Arc<Vec<u8>>> {
        if let Some(v) = self.lru.map.get(&id) {
            let v = v.clone();
            self.lru.order.retain(|&x| x != id);
            self.lru.order.push_back(id);
            return Ok(v);
        }
        let v = Arc::new(self.ar.read_block(id, true)?);
        self.lru.map.insert(id, v.clone());
        self.lru.order.push_back(id);
        while self.lru.order.len() > self.lru.cap {
            if let Some(old) = self.lru.order.pop_front() {
                self.lru.map.remove(&old);
            }
        }
        Ok(v)
    }
}

/// An opened archive.
#[wasm_bindgen]
pub struct EzpzArchive {
    ar: Archive,
    lru: RefCell<BlockLru>,
}

#[wasm_bindgen]
impl EzpzArchive {
    /// Opens archive bytes and checks the header, index, and signature. An encrypted archive
    /// needs `password`; without it the archive opens for checking only (`needsPassword()`).
    /// A wrong password throws.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: Vec<u8>, password: Option<String>) -> Result<EzpzArchive, JsError> {
        let ar = Archive::from_bytes(bytes, password.as_deref().map(str::as_bytes)).map_err(js_err)?;
        Ok(EzpzArchive {
            ar,
            lru: RefCell::new(BlockLru {
                map: HashMap::new(),
                order: VecDeque::new(),
                cap: 2,
            }),
        })
    }

    #[wasm_bindgen(js_name = isEncrypted)]
    pub fn is_encrypted(&self) -> bool {
        self.ar.header.encrypted()
    }

    /// True for an encrypted archive opened without a password.
    #[wasm_bindgen(js_name = needsPassword)]
    pub fn needs_password(&self) -> bool {
        !self.ar.has_catalog()
    }

    #[wasm_bindgen(js_name = isSigned)]
    pub fn is_signed(&self) -> bool {
        self.ar.signature.is_some()
    }

    /// Ed25519 public key of a signed archive, in hex (the signature was already checked).
    #[wasm_bindgen(js_name = signerKey)]
    pub fn signer_key(&self) -> Option<String> {
        self.ar.signature.as_ref().map(|s| hex(&s.public_key))
    }

    /// Archive fingerprint (root hash), in hex.
    pub fn digest(&self) -> String {
        hex(&self.ar.digest)
    }

    /// JSON array of entries, sorted by path:
    /// `[{"path", "type": "file"|"dir"|"symlink", "size", "mode", "mtime", "block", "target"}]`.
    /// `mtime` is in Unix seconds. `block` is the first block a file uses: reading files in
    /// that order decodes each block once.
    #[wasm_bindgen(js_name = entriesJson)]
    pub fn entries_json(&self) -> Result<String, JsError> {
        self.need_catalog()?;
        let cm = self.ar.chunk_map();
        let mut out = String::from("[");
        for (i, e) in self.ar.catalog().entries.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let mtime = e.mtime_s as f64 + e.mtime_ns as f64 / 1e9;
            let _ = write!(out, "{{\"path\":{},\"mode\":{},\"mtime\":{}", json_str(&e.path), e.mode, mtime);
            match &e.kind {
                Kind::File { chunks, .. } => {
                    let block = chunks.first().map(|&c| cm.block[c as usize] as i64).unwrap_or(-1);
                    let _ = write!(out, ",\"type\":\"file\",\"size\":{},\"block\":{}", cm.file_size(chunks), block);
                }
                Kind::Dir => out.push_str(",\"type\":\"dir\",\"size\":0"),
                Kind::Symlink { target } => {
                    let _ = write!(out, ",\"type\":\"symlink\",\"size\":0,\"target\":{}", json_str(target));
                }
            }
            out.push('}');
        }
        out.push(']');
        Ok(out)
    }

    /// JSON summary: `{"encrypted", "signed", "blocks": {codec name: count}, "storedBytes",
    /// "files", "dirs", "links", "contentBytes", "creator", "created"}`. Without the password
    /// of an encrypted archive, only the first four are filled in.
    #[wasm_bindgen(js_name = infoJson)]
    pub fn info_json(&self) -> String {
        let mut per: BTreeMap<&str, u64> = BTreeMap::new();
        let mut stored = 0u64;
        for (i, b) in self.ar.table.blocks.iter().enumerate() {
            stored += b.stored_len as u64;
            let name = self.block_codec(i as u32).map(codec_name).unwrap_or("unknown");
            *per.entry(name).or_default() += 1;
        }
        let mut out = format!(
            "{{\"encrypted\":{},\"signed\":{},\"storedBytes\":{},\"blocks\":{{",
            self.ar.header.encrypted(),
            self.ar.signature.is_some(),
            stored
        );
        for (i, (k, v)) in per.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "{}:{}", json_str(k), v);
        }
        out.push('}');
        if self.ar.has_catalog() {
            let cat = self.ar.catalog();
            let cm = self.ar.chunk_map();
            let (mut files, mut dirs, mut links, mut bytes) = (0u64, 0u64, 0u64, 0u64);
            for e in &cat.entries {
                match &e.kind {
                    Kind::File { chunks, .. } => {
                        files += 1;
                        bytes += cm.file_size(chunks);
                    }
                    Kind::Dir => dirs += 1,
                    Kind::Symlink { .. } => links += 1,
                }
            }
            let _ = write!(
                out,
                ",\"files\":{files},\"dirs\":{dirs},\"links\":{links},\"contentBytes\":{bytes},\"creator\":{},\"created\":{}",
                json_str(&cat.creator),
                cat.created
            );
        }
        out.push('}');
        out
    }

    /// Content of one file (hash checked). Throws if `path` is not a file in the archive.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, JsError> {
        self.need_catalog()?;
        let e: &Entry = self
            .ar
            .find(path)
            .ok_or_else(|| JsError::new(&format!("not found in archive: {path}")))?;
        let mut lru = self.lru.borrow_mut();
        let mut src = Cached { ar: &self.ar, lru: &mut lru };
        let mut out = Vec::new();
        write_content(&self.ar, &mut src, e, &mut out).map_err(js_err)?;
        Ok(out)
    }

    /// Checks every block hash and, if the archive could be opened fully, every file hash.
    /// Returns JSON: `{"ok", "blocksOk", "blocksBad": [...], "filesOk", "filesBad": [...],
    /// "contentChecked"}`.
    #[wasm_bindgen(js_name = verifyJson)]
    pub fn verify_json(&self) -> String {
        let rep = ezpz::archive::verify(&self.ar, 1);
        let list = |v: &[String]| -> String {
            let parts: Vec<String> = v.iter().map(|s| json_str(s)).collect();
            format!("[{}]", parts.join(","))
        };
        format!(
            "{{\"ok\":{},\"blocksOk\":{},\"blocksBad\":{},\"filesOk\":{},\"filesBad\":{},\"contentChecked\":{}}}",
            rep.blocks_bad.is_empty() && rep.files_bad.is_empty(),
            rep.blocks_ok,
            list(&rep.blocks_bad),
            rep.files_ok,
            list(&rep.files_bad),
            rep.content_checked
        )
    }

    /// Frees the decoded blocks kept between `read` calls.
    #[wasm_bindgen(js_name = clearCache)]
    pub fn clear_cache(&self) {
        let mut l = self.lru.borrow_mut();
        l.map.clear();
        l.order.clear();
    }
}

impl EzpzArchive {
    fn need_catalog(&self) -> Result<(), JsError> {
        if self.ar.has_catalog() {
            Ok(())
        } else {
            Err(JsError::new("password required"))
        }
    }

    /// Codec byte from the block's frame header (hash-checked read without decoding).
    fn block_codec(&self, id: u32) -> Option<u8> {
        self.ar.block_codec(id).ok()
    }
}

/// Creates an archive and returns its bytes.
///
/// `files`: array of `{path: string, data?: Uint8Array, dir?: boolean, target?: string,
/// mtime?: number (milliseconds, like Date.now()), mode?: number}`. Paths use '/' and may
/// include folders ("photos/2026/a.jpg"); folders that are not listed are added.
///
/// `options`: `{level?: 1..11 (default 7), password?: string, dedup?: boolean,
/// filters?: boolean, hashLen?: 0|16|32}`. Levels 10 (`--max`) and 11 use the brain codecs,
/// which are slow in a browser too.
///
/// `onProgress(done, total)`: called after each block with input bytes.
#[wasm_bindgen]
pub fn create(files: js_sys::Array, options: JsValue, on_progress: Option<js_sys::Function>) -> Result<Vec<u8>, JsError> {
    let get = |o: &JsValue, k: &str| js_sys::Reflect::get(o, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED);
    let now_ms = js_sys::Date::now();
    let mut entries = Vec::with_capacity(files.length() as usize);
    for f in files.iter() {
        let path = get(&f, "path")
            .as_string()
            .ok_or_else(|| JsError::new("each file needs a string \"path\""))?;
        let mtime_ms = get(&f, "mtime").as_f64().unwrap_or(now_ms);
        let mode = get(&f, "mode").as_f64().unwrap_or(0.0) as u32;
        let kind = if get(&f, "dir").as_bool().unwrap_or(false) {
            MemKind::Dir
        } else if let Some(t) = get(&f, "target").as_string() {
            MemKind::Symlink(t)
        } else {
            let d = get(&f, "data");
            if d.is_undefined() || d.is_null() {
                MemKind::File(Vec::new())
            } else {
                MemKind::File(js_sys::Uint8Array::new(&d).to_vec())
            }
        };
        let secs = (mtime_ms / 1000.0).floor();
        let ns = ((mtime_ms - secs * 1000.0) * 1e6).round().clamp(0.0, 999_999_999.0) as u32;
        entries.push(MemEntry {
            path,
            kind,
            mode,
            mtime_s: secs as i64,
            mtime_ns: ns,
        });
    }

    let level = get(&options, "level").as_f64().map(|l| l as u8).unwrap_or(create::DEFAULT_LEVEL);
    if !(1..=create::MAX_LEVEL).contains(&level) {
        return Err(JsError::new(&format!("level must be 1 to {}", create::MAX_LEVEL)));
    }
    let hash_len = get(&options, "hashLen").as_f64().map(|h| h as u8).unwrap_or(16);
    if ![0, 16, 32].contains(&hash_len) {
        return Err(JsError::new("hashLen must be 0, 16, or 32"));
    }
    let password = get(&options, "password").as_string().filter(|p| !p.is_empty());
    let o = CreateOptions {
        level,
        codec: CodecChoice::Auto,
        brain_mask: None,
        block_size: None,
        threads: 1,
        password: password.map(String::into_bytes),
        signing_key: None,
        hash_len,
        dedup: get(&options, "dedup").as_bool().unwrap_or(true),
        filters: get(&options, "filters").as_bool().unwrap_or(true),
        verbose: false,
        created: Some((now_ms / 1000.0).floor() as i64),
    };
    let mut report = |done: u64, total: u64| {
        if let Some(f) = &on_progress {
            let _ = f.call2(&JsValue::NULL, &JsValue::from_f64(done as f64), &JsValue::from_f64(total as f64));
        }
    };
    let (bytes, _) = create::create_in_memory(entries, &o, Some(&mut report)).map_err(js_err)?;
    Ok(bytes)
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        let _ = write!(s, "{x:02x}");
    }
    s
}

/// JSON string literal.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
