//! Encoder-side heuristics: group similar files, skip already-compressed data,
//! detect machine code. None of this is needed to *read* an archive.

use crate::filter::{XF_ARM64, XF_NONE, XF_X86, XF_X86_64_SPLIT};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Class {
    Compressible = 0,
    Exec = 1,
    Incompressible = 2,
}

const STORE_EXT: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "avif", "heic", "heif", "jxl", "mp4", "m4v", "mov", "mkv",
    "webm", "avi", "mp3", "m4a", "aac", "ogg", "oga", "opus", "flac", "zip", "gz", "tgz", "xz",
    "txz", "zst", "bz2", "tbz2", "lz4", "7z", "rar", "br", "jar", "apk", "ipa", "aab", "docx",
    "xlsx", "pptx", "odt", "ods", "odp", "epub", "woff", "woff2", "ezpz", "cab", "whl", "nupkg",
    "crx", "xpi", "lzma", "zipx",
];

const TEXT_EXT: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "rst",
    "c",
    "h",
    "cc",
    "cpp",
    "hpp",
    "cxx",
    "rs",
    "go",
    "py",
    "pyi",
    "js",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "jsx",
    "json",
    "html",
    "htm",
    "css",
    "scss",
    "less",
    "xml",
    "csv",
    "tsv",
    "log",
    "sql",
    "java",
    "kt",
    "swift",
    "rb",
    "php",
    "sh",
    "bash",
    "zsh",
    "yml",
    "yaml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "svg",
    "tex",
    "vue",
    "svelte",
    "lua",
    "pl",
    "pm",
    "r",
    "m",
    "mm",
    "cs",
    "fs",
    "dart",
    "scala",
    "ex",
    "exs",
    "erl",
    "hs",
    "ml",
    "clj",
    "el",
    "vim",
    "gradle",
    "properties",
    "lock",
    "srt",
    "vtt",
    "po",
    "pot",
];

pub fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

fn detect_exec(s: &[u8]) -> Option<u8> {
    if s.len() >= 20 && &s[..4] == b"\x7fELF" {
        let m = if s[5] == 2 {
            u16::from_be_bytes([s[18], s[19]])
        } else {
            u16::from_le_bytes([s[18], s[19]])
        };
        return Some(match m {
            0x03 => XF_X86,
            0x3E => XF_X86_64_SPLIT,
            0xB7 => XF_ARM64,
            _ => XF_NONE,
        });
    }
    if s.len() >= 8 && (s[..4] == [0xCF, 0xFA, 0xED, 0xFE] || s[..4] == [0xCE, 0xFA, 0xED, 0xFE]) {
        let cpu = u32::from_le_bytes([s[4], s[5], s[6], s[7]]);
        return Some(match (cpu, cpu & 0x00FF_FFFF) {
            (0x0100_0007, _) => XF_X86_64_SPLIT,
            (_, 7) => XF_X86,
            (_, 12) => XF_ARM64,
            _ => XF_NONE,
        });
    }
    if s.len() >= 0x40 && &s[..2] == b"MZ" {
        let e = u32::from_le_bytes([s[0x3C], s[0x3D], s[0x3E], s[0x3F]]) as usize;
        if e + 6 <= s.len() && &s[e..e + 4] == b"PE\0\0" {
            let m = u16::from_le_bytes([s[e + 4], s[e + 5]]);
            return Some(match m {
                0x014C => XF_X86,
                0x8664 => XF_X86_64_SPLIT,
                0xAA64 => XF_ARM64,
                _ => XF_NONE,
            });
        }
    }
    None
}

fn compressed_magic(s: &[u8]) -> bool {
    let starts = |m: &[u8]| s.len() >= m.len() && &s[..m.len()] == m;
    starts(&[0x1F, 0x8B])
        || starts(&[0x28, 0xB5, 0x2F, 0xFD])
        || starts(&[0xFD, b'7', b'z', b'X', b'Z', 0])
        || starts(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C])
        || starts(b"Rar!")
        || starts(b"PK\x03\x04")
        || starts(&[0x89, b'P', b'N', b'G'])
        || starts(&[0xFF, 0xD8, 0xFF])
        || starts(b"GIF8")
        || starts(b"OggS")
        || starts(b"fLaC")
        || starts(&crate::format::MAGIC)
        || (s.len() >= 12 && &s[4..8] == b"ftyp")
}

/// Returns (class, transform).
pub fn classify(rel: &str, sample: &[u8], use_filters: bool) -> (Class, u8) {
    let ext = extension(rel);
    if let Some(xf) = detect_exec(sample) {
        return (Class::Exec, if use_filters { xf } else { XF_NONE });
    }
    if STORE_EXT.contains(&ext.as_str()) || compressed_magic(sample) {
        return (Class::Incompressible, XF_NONE);
    }
    if TEXT_EXT.contains(&ext.as_str()) {
        return (Class::Compressible, XF_NONE);
    }
    if crate::codec::looks_incompressible(sample) {
        return (Class::Incompressible, XF_NONE);
    }
    (Class::Compressible, XF_NONE)
}
