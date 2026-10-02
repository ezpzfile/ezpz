use ezpz::{archive, brain, create, filter, format, index};
use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use create::CodecChoice;
use format::codec_name;
use index::Kind;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "ezpz",
    version,
    about = "EZPZ archiver - reference implementation of the .ezpz format"
)]
struct Cli {
    /// Worker threads (default: all cores)
    #[arg(short = 'j', long, global = true)]
    threads: Option<usize>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum CodecArg {
    Auto,
    Zstd,
    Lzma2,
    Brain,
    BrainFast,
    Store,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create an archive
    #[command(alias = "c")]
    Create {
        archive: PathBuf,
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// 1 (fastest) .. 9 (smallest of the fast-to-extract levels, zstd/LZMA2),
        /// 10 = max (fast brain codec), 11 = smallest (brain codec, slowest). Default 7
        #[arg(short, long, default_value_t = create::DEFAULT_LEVEL, value_parser = clap::value_parser!(u8).range(1..=create::MAX_LEVEL as i64))]
        level: u8,
        /// Same as --level 10: the fast "brain" codec. Strongest on text, slow to extract (-l 11 is smaller still)
        #[arg(long)]
        max: bool,
        #[arg(long, value_enum, default_value_t = CodecArg::Auto)]
        codec: CodecArg,
        /// Solid block size in MiB (default depends on level)
        #[arg(long)]
        block_size: Option<usize>,
        /// (testing) fixed brain-fast model mask in hex, e.g. 0x1cb
        #[arg(long, hide = true, value_parser = parse_hex_u16)]
        brain_mask: Option<u16>,
        /// Encrypt (asks for a password, or uses --password / EZPZ_PASSWORD)
        #[arg(short, long)]
        encrypt: bool,
        #[arg(long)]
        password: Option<String>,
        /// Sign with an Ed25519 secret key file (see `ezpz keygen`)
        #[arg(long)]
        sign: Option<PathBuf>,
        /// Per-file hash length in bytes: 0, 16 or 32
        #[arg(long, default_value_t = 16)]
        hash_len: u8,
        #[arg(long)]
        no_dedup: bool,
        #[arg(long)]
        no_filter: bool,
        #[arg(short, long)]
        verbose: bool,
        /// Print a machine-readable summary line
        #[arg(long)]
        json: bool,
    },
    /// Extract all files, or only the given paths
    #[command(alias = "x")]
    Extract {
        archive: PathBuf,
        paths: Vec<String>,
        #[arg(short = 'C', long, default_value = ".")]
        dir: PathBuf,
        #[arg(short, long)]
        force: bool,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        unsafe_links: bool,
        #[arg(short, long)]
        verbose: bool,
    },
    /// List contents
    #[command(alias = "l")]
    List {
        archive: PathBuf,
        /// Show each file's BLAKE3 hash prefix
        #[arg(long)]
        hash: bool,
        #[arg(long)]
        password: Option<String>,
    },
    /// Write one file to stdout (random access - only its blocks are read)
    Cat {
        archive: PathBuf,
        path: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// Check integrity (and signature). Without the password, encrypted archives are checked structurally.
    #[command(alias = "t")]
    Verify {
        archive: PathBuf,
        /// Require a signature from this public key (hex or .pub file)
        #[arg(long)]
        pubkey: Option<String>,
        #[arg(long)]
        password: Option<String>,
        /// For encrypted archives: don't ask for a password, check structure only
        #[arg(long)]
        no_password: bool,
    },
    /// Show archive details
    Info {
        archive: PathBuf,
        #[arg(long)]
        password: Option<String>,
        /// Also list every block (codec, size before and after compression)
        #[arg(long)]
        blocks: bool,
    },
    /// Generate an Ed25519 signing key pair: NAME.key (secret) and NAME.pub
    Keygen { name: PathBuf },
    /// (developer) brain-codec size/speed on raw files
    #[command(hide = true)]
    Tune {
        files: Vec<PathBuf>,
        #[arg(long)]
        check: bool,
        /// Use the fast profile (codec 4)
        #[arg(long)]
        fast: bool,
        /// With --fast: fixed model mask in hex instead of the per-block choice
        #[arg(long, value_parser = parse_hex_u16)]
        mask: Option<u16>,
    },
}

fn parse_hex_u16(s: &str) -> Result<u16, String> {
    u16::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| e.to_string())
}

/// Passwords are NFC-normalized UTF-8 (SPEC §13), so the same Hangul password typed on
/// different systems (composed vs. decomposed jamo) always gives the same key.
fn nfc_bytes(p: &str) -> Vec<u8> {
    use unicode_normalization::UnicodeNormalization;
    p.nfc().collect::<String>().into_bytes()
}

fn password_source(cli: Option<String>, confirm: bool) -> impl FnMut() -> Result<Vec<u8>> {
    move || {
        if let Some(p) = &cli {
            return Ok(nfc_bytes(p));
        }
        if let Ok(p) = std::env::var("EZPZ_PASSWORD") {
            return Ok(nfc_bytes(&p));
        }
        let p = rpassword::prompt_password("Password: ")?;
        if confirm {
            let q = rpassword::prompt_password("Repeat password: ")?;
            ensure!(p == q, "passwords do not match");
        }
        ensure!(!p.is_empty(), "empty password");
        Ok(nfc_bytes(&p))
    }
}

fn read_hex_file_or_str(s: &str) -> Result<Vec<u8>> {
    let text = if Path::new(s).exists() {
        std::fs::read_to_string(s)?
    } else {
        s.to_string()
    };
    hex::decode(text.trim()).map_err(|_| anyhow!("expected hex key in {s}"))
}

fn human(n: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.2} {}", units[u])
    }
}

fn civil(ts: i64) -> String {
    let days = ts.div_euclid(86400);
    let secs = ts.rem_euclid(86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60
    )
}

fn main() {
    if let Err(e) = run() {
        eprintln!("ezpz: error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let threads = cli.threads.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    });
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global()
        .ok();

    match cli.cmd {
        Cmd::Create {
            archive,
            inputs,
            level,
            max,
            codec,
            block_size,
            brain_mask,
            encrypt,
            password,
            sign,
            hash_len,
            no_dedup,
            no_filter,
            verbose,
            json,
        } => {
            ensure!(
                matches!(hash_len, 0 | 16 | 32),
                "--hash-len must be 0, 16 or 32"
            );
            let password = if encrypt || password.is_some() {
                Some(password_source(password, true)()?)
            } else {
                None
            };
            let signing_key = match sign {
                Some(p) => {
                    let b = read_hex_file_or_str(p.to_str().unwrap_or_default())?;
                    let arr: [u8; 32] = b
                        .try_into()
                        .map_err(|_| anyhow!("secret key must be 32 bytes"))?;
                    Some(ed25519_dalek::SigningKey::from_bytes(&arr))
                }
                None => None,
            };
            let opts = create::CreateOptions {
                level: if max { 10 } else { level },
                codec: match codec {
                    CodecArg::Auto => CodecChoice::Auto,
                    CodecArg::Zstd => CodecChoice::Zstd,
                    CodecArg::Lzma2 => CodecChoice::Lzma2,
                    CodecArg::Brain => CodecChoice::Brain,
                    CodecArg::BrainFast => CodecChoice::BrainFast,
                    CodecArg::Store => CodecChoice::Store,
                },
                block_size: block_size.map(|m| m << 20),
                brain_mask,
                threads,
                password,
                signing_key,
                hash_len,
                dedup: !no_dedup,
                filters: !no_filter,
                verbose,
                created: None,
            };
            let s = create::create(&archive, &inputs, &opts)?;
            if json {
                println!(
                    "{{\"files\":{},\"input_bytes\":{},\"archive_bytes\":{},\"dedup_bytes\":{},\"seconds\":{:.3},\"blocks\":{}}}",
                    s.files, s.input_bytes, s.archive_bytes, s.dedup_bytes, s.seconds, s.blocks
                );
            } else {
                let ratio = if s.input_bytes > 0 {
                    s.archive_bytes as f64 * 100.0 / s.input_bytes as f64
                } else {
                    0.0
                };
                eprintln!(
                    "{}: {} files, {} dirs, {} links | {} -> {} ({ratio:.1}%) in {:.2}s",
                    archive.display(),
                    s.files,
                    s.dirs,
                    s.symlinks,
                    human(s.input_bytes),
                    human(s.archive_bytes),
                    s.seconds
                );
                if s.dedup_bytes > 0 {
                    eprintln!(
                        "  duplicate data stored once: {} saved",
                        human(s.dedup_bytes)
                    );
                }
                let c = s.codec_blocks;
                eprintln!(
                    "  {} blocks (store {}, zstd {}, lzma2 {}, brain {}, brain-fast {}, zstd-primed {})",
                    s.blocks, c[0], c[1], c[2], c[3], c[4], c[5]
                );
            }
        }

        Cmd::Extract {
            archive,
            paths,
            dir,
            force,
            password,
            unsafe_links,
            verbose,
        } => {
            let ar = archive::Archive::open(&archive, &mut password_source(password, false), true)?;
            let st = archive::extract(
                &ar,
                &paths,
                &archive::ExtractOptions {
                    dest: dir,
                    force,
                    threads,
                    unsafe_links,
                    verbose,
                },
            )?;
            eprintln!(
                "extracted {} files ({}), all hashes verified",
                st.files,
                human(st.bytes)
            );
        }

        Cmd::List {
            archive,
            hash,
            password,
        } => {
            let ar = archive::Archive::open(&archive, &mut password_source(password, false), true)?;
            let cat = ar.catalog();
            let cm = ar.chunk_map();
            let out = std::io::stdout();
            let mut out = out.lock();
            let mut total = 0u64;
            let mut nfiles = 0u64;
            for e in &cat.entries {
                let (t, size, extra) = match &e.kind {
                    Kind::File {
                        chunks, hash: h, ..
                    } => {
                        let s = cm.file_size(chunks);
                        total += s;
                        nfiles += 1;
                        (
                            '-',
                            s,
                            if hash {
                                format!("  {}", hex::encode(h))
                            } else {
                                String::new()
                            },
                        )
                    }
                    Kind::Dir => ('d', 0, String::new()),
                    Kind::Symlink { target } => ('l', 0, format!(" -> {target}")),
                };
                let _ = writeln!(
                    out,
                    "{t}{:04o} {:>12} {}  {}{}",
                    e.mode,
                    size,
                    civil(e.mtime_s),
                    e.path,
                    extra
                );
            }
            let _ = writeln!(
                out,
                "{} files, {} total, archive {}",
                nfiles,
                human(total),
                human(ar.len)
            );
        }

        Cmd::Cat {
            archive,
            path,
            password,
        } => {
            let ar = archive::Archive::open(&archive, &mut password_source(password, false), true)?;
            let e = ar
                .find(&path)
                .ok_or_else(|| anyhow!("not found in archive: {path}"))?;
            let seq = archive::access_sequence(&ar, &[e]);
            let mut cache = archive::BlockCache::new(&ar, seq, threads);
            let out = std::io::stdout();
            let mut out = std::io::BufWriter::new(out.lock());
            archive::write_content(&ar, &mut cache, e, &mut out)?;
            out.flush()?;
        }

        Cmd::Verify {
            archive,
            pubkey,
            password,
            no_password,
        } => {
            let need = !no_password;
            let ar = archive::Archive::open(&archive, &mut password_source(password, false), need)?;
            println!("structure: OK (header, index, trailer, root hash)");
            match &ar.signature {
                Some(s) => {
                    println!("signature: VALID, signed by {}", hex::encode(s.public_key));
                    if let Some(pk) = pubkey {
                        let want = read_hex_file_or_str(&pk)?;
                        ensure!(
                            want == s.public_key,
                            "signature is valid but from a DIFFERENT key than expected"
                        );
                        println!("signer:    matches the expected public key");
                    }
                }
                None => {
                    if pubkey.is_some() {
                        bail!("archive is not signed");
                    }
                    println!(
                        "signature: none (catches accidental damage; sign with --sign to make tampering detectable)"
                    );
                }
            }
            let rep = archive::verify(&ar, threads);
            println!(
                "blocks:    {} OK, {} bad",
                rep.blocks_ok,
                rep.blocks_bad.len()
            );
            for b in &rep.blocks_bad {
                println!("  ! {b}");
            }
            if !rep.blocks_bad.is_empty() {
                println!("files:     not checked (damaged blocks)");
            } else if rep.content_checked {
                println!(
                    "files:     {} OK, {} bad",
                    rep.files_ok,
                    rep.files_bad.len()
                );
                for b in &rep.files_bad {
                    println!("  ! {b}");
                }
            } else {
                println!("files:     not checked (encrypted; give the password for a full check)");
            }
            println!("digest:    {}", hex::encode(ar.digest));
            if !rep.blocks_bad.is_empty() || !rep.files_bad.is_empty() {
                bail!("archive is damaged");
            }
            println!("result:    OK");
        }

        Cmd::Info { archive, password, blocks } => {
            let ar = archive::Archive::open(&archive, &mut password_source(password, false), true)?;
            let h = &ar.header;
            println!(
                "format:      .ezpz {}.{}",
                format::VERSION_MAJOR,
                h.version_minor
            );
            println!("archive id:  {}", hex::encode(h.archive_id));
            println!("digest:      {}", hex::encode(ar.digest));
            println!("size:        {}", human(ar.len));
            println!(
                "encryption:  {}",
                match &h.encryption {
                    Some(e) => format!(
                        "XChaCha20-Poly1305, Argon2id (m={} MiB, t={}, p={})",
                        e.m_kib / 1024,
                        e.t_cost,
                        e.p_lanes
                    ),
                    None => "none".into(),
                }
            );
            println!(
                "signature:   {}",
                match &ar.signature {
                    Some(s) => format!("Ed25519 {}", hex::encode(s.public_key)),
                    None => "none".into(),
                }
            );
            let mut per = std::collections::BTreeMap::new();
            let mut raw_total = 0u64;
            for b in &ar.table.blocks {
                raw_total += b.raw_len as u64;
            }
            for i in 0..ar.table.blocks.len() {
                let mut fh = [0u8; 16];
                // codec comes from each frame header
                let f = std::fs::File::open(&archive)?;
                read_exact_at(&f, &mut fh, ar.block_offsets[i])?;
                if blocks {
                    let stored = u32::from_le_bytes([fh[12], fh[13], fh[14], fh[15]]);
                    println!(
                        "  block {i}: {} {} -> {}",
                        codec_name(fh[4]),
                        ar.table.blocks[i].raw_len,
                        stored
                    );
                }
                let e = per.entry(codec_name(fh[4])).or_insert((0u64, 0u64, 0u64));
                e.0 += 1;
                e.1 += ar.table.blocks[i].raw_len as u64;
                e.2 += ar.table.blocks[i].stored_len as u64;
            }
            println!("blocks:      {}", ar.table.blocks.len());
            for (k, (n, r, s)) in per {
                println!("  {k:<11} {n:>5} blocks  {} -> {}", human(r), human(s));
            }
            let cat = ar.catalog();
            let cm = ar.chunk_map();
            let mut logical = 0u64;
            let (mut nf, mut nd, mut nl) = (0, 0, 0);
            let mut xf_count = [0u64; filter::XF_MAX as usize + 1];
            for e in &cat.entries {
                match &e.kind {
                    Kind::File { chunks, transform, .. } => {
                        nf += 1;
                        logical += cm.file_size(chunks);
                        xf_count[(*transform).min(filter::XF_MAX) as usize] += 1;
                    }
                    Kind::Dir => nd += 1,
                    Kind::Symlink { .. } => nl += 1,
                }
            }
            println!("entries:     {nf} files, {nd} dirs, {nl} symlinks");
            let xf_names = ["none", "x86", "arm64", "x86-64", "x86-64 split"];
            let used: Vec<String> = (1..xf_count.len())
                .filter(|&t| xf_count[t] > 0)
                .map(|t| format!("{} {} files", xf_names[t], xf_count[t]))
                .collect();
            if !used.is_empty() {
                println!("transforms:  {}", used.join(", "));
            }
            println!(
                "content:     {} (unique after dedup: {})",
                human(logical),
                human(raw_total)
            );
            println!(
                "index:       table {} + catalog {}",
                human(ar.table_frame.stored_len as u64 + 16),
                human(ar.catalog_frame.stored_len as u64 + 16)
            );
            println!("created by:  {} at {} UTC", cat.creator, civil(cat.created));
            let _ = ar.catalog_offset();
        }

        Cmd::Tune { files, check, fast, mask } => {
            let profile = match (fast, mask) {
                (false, _) => brain::Profile::Full,
                (true, None) => brain::Profile::Fast,
                (true, Some(m)) => brain::Profile::FastMask(m),
            };
            use rayon::prelude::*;
            let res: Vec<(String, usize, f64, f64, bool, String)> = files
                .par_iter()
                .map(|f| {
                    let d = std::fs::read(f).unwrap();
                    let t = std::time::Instant::now();
                    let c = brain::compress(&d, profile);
                    let secs = t.elapsed().as_secs_f64();
                    let t2 = std::time::Instant::now();
                    let ok = !check
                        || brain::decompress(&c, d.len(), profile)
                            .map(|x| x == d)
                            .unwrap_or(false);
                    let dsecs = if check { t2.elapsed().as_secs_f64() } else { 0.0 };
                    let h = hex::encode(&blake3::hash(&c).as_bytes()[..6]);
                    (f.display().to_string(), c.len(), secs, dsecs, ok, h)
                })
                .collect();
            let mut total = 0;
            for (f, n, s, ds, ok, h) in &res {
                total += n;
                println!(
                    "{f:<24} {n:>10} {s:>7.2}s {ds:>7.2}s {h} {}",
                    if *ok { "" } else { "ROUNDTRIP FAIL" }
                );
            }
            println!("TOTAL {total}");
        }

        Cmd::Keygen { name } => {
            let sk = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
            let kp = name.with_extension("key");
            let pp = name.with_extension("pub");
            ensure!(!kp.exists(), "{} already exists", kp.display());
            std::fs::write(&kp, hex::encode(sk.to_bytes()) + "\n").context("writing secret key")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&kp, std::fs::Permissions::from_mode(0o600))?;
            }
            std::fs::write(&pp, hex::encode(sk.verifying_key().to_bytes()) + "\n")?;
            println!(
                "secret key: {} (keep private)\npublic key: {}",
                kp.display(),
                pp.display()
            );
        }
    }
    Ok(())
}

fn read_exact_at(f: &std::fs::File, buf: &mut [u8], off: u64) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = f;
    f.seek(SeekFrom::Start(off))?;
    f.read_exact(buf)
}
