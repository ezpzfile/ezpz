#!/usr/bin/env bash
# Builds the WebAssembly package (web/pkg) and the single-file page (web/dist/ezpz.html).
#
# Needs Rust with the wasm32-unknown-unknown target (rustup target add wasm32-unknown-unknown),
# a clang that can target wasm32 (zstd is C code; on macOS use Homebrew's llvm, because
# Apple's clang has no WebAssembly backend), wasm-bindgen-cli 0.2.129
# (cargo install wasm-bindgen-cli --version 0.2.129), and python3.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -z "${CC_wasm32_unknown_unknown:-}" ]; then
  for c in /opt/homebrew/opt/llvm/bin/clang /usr/local/opt/llvm/bin/clang clang; do
    if command -v "$c" >/dev/null 2>&1; then export CC_wasm32_unknown_unknown="$c"; break; fi
  done
fi
if [ -z "${AR_wasm32_unknown_unknown:-}" ]; then
  for a in /opt/homebrew/opt/llvm/bin/llvm-ar /usr/local/opt/llvm/bin/llvm-ar llvm-ar; do
    if command -v "$a" >/dev/null 2>&1; then export AR_wasm32_unknown_unknown="$a"; break; fi
  done
fi

cargo build --release --target wasm32-unknown-unknown -p ezpz-wasm
wasm-bindgen --target web --out-dir web/pkg target/wasm32-unknown-unknown/release/ezpz_wasm.wasm
wasm-bindgen --target no-modules --no-typescript --out-dir target/web-classic target/wasm32-unknown-unknown/release/ezpz_wasm.wasm
python3 web/inline.py target/web-classic
ls -l web/pkg/ezpz_wasm_bg.wasm web/dist/ezpz.html
