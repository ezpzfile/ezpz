//! Reference implementation of the `.ezpz` archive format (SPEC.md).
//!
//! The command-line tool (`src/main.rs`) and the WebAssembly package (`wasm/`) are both built
//! on this library. Reading works from a file or from bytes in memory ([`archive::Archive`]),
//! and archives can be created from files on disk or from a list of files in memory
//! ([`create::create_in_memory`]). The WebAssembly build has no file system, no threads, and no
//! LZMA2 encoder (it decodes LZMA2 blocks, but level 9 compresses with zstd only).

pub mod archive;
pub mod brain;
pub mod classify;
pub mod codec;
pub mod create;
pub mod crypto;
pub mod filter;
pub mod format;
pub mod index;
