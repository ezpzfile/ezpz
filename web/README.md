# ezpz in a browser

English | [한국어](README.ko.md)

This folder holds a WebAssembly build of ezpz and a page that uses it to open, extract, and create `.ezpz` archives in a browser. Files never leave the device: there is no server side at all.

## Try it

- **`dist/ezpz.html`** is one file with everything inside (about 1.7 MB). Save it and open it from the disk.
- **`index.html`** is the same page, loading `worker.js` and `pkg/`. Serve this folder with any static web server, for example `python3 -m http.server -d web`, then open http://localhost:8000/.

The page can:

- open an archive, list its files, and check it for damage
- save one file, or all of them as a `.zip`
- open encrypted archives with their password, and show who signed an archive
- create an archive from files and folders (drag and drop works for folders too), at four levels and with an optional password

It is in English and Korean and follows the system's light or dark setting.

## Differences from the command-line tool

- **No LZMA2 encoder.** Level 9 compresses with zstd alone. LZMA2 blocks written by the command-line tool are still decoded, with a pure-Rust decoder.
- **One core.** Blocks are compressed one after another (about 2 MB per second at the default level on a test server). The page does the work in a Web Worker, so it stays responsive.
- **In memory.** The archive and its files have to fit in the browser's memory. For archives of several GB, use the command-line tool.
- **The brain codecs are slow here too.** On the same server, level 10 handled about 0.8 MB per second and level 11 about 0.6 MB per second, in both directions.

At every level except 9, the blocks it writes are byte for byte the same as the command-line tool's.

## Files

| File | Purpose |
|---|---|
| `index.html` | the page |
| `worker.js` | the Web Worker; its message protocol is described at the top of the file |
| `pkg/` | wasm-bindgen output (`--target web`): `ezpz_wasm.js`, `ezpz_wasm_bg.wasm`, and TypeScript types |
| `dist/ezpz.html` | the single-file page, made by `inline.py` |
| `build.sh` | builds `pkg/` and `dist/` |
| `test.mjs` | tests the WebAssembly build in Node: `node web/test.mjs` |

## JavaScript API

```js
import init, { EzpzArchive, create, version } from './pkg/ezpz_wasm.js';
await init(); // loads ezpz_wasm_bg.wasm from next to ezpz_wasm.js

// Open, list, extract
const archive = new EzpzArchive(new Uint8Array(await file.arrayBuffer()), password); // password is optional
if (archive.needsPassword()) {
  // encrypted, and no password was given: open it again with one
}
const entries = JSON.parse(archive.entriesJson()); // [{path, type, size, mode, mtime, block, target}]
const data = archive.read('photos/a.jpg');          // Uint8Array, hash checked
const report = JSON.parse(archive.verifyJson());    // {ok, blocksOk, blocksBad, filesOk, filesBad, contentChecked}
const info = JSON.parse(archive.infoJson());        // {encrypted, signed, blocks, storedBytes, files, dirs, links, contentBytes, creator, created}
archive.free();

// Create
const bytes = create(
  [
    { path: 'notes/today.txt', data: new TextEncoder().encode('hello'), mtime: Date.now() },
    { path: 'notes/drafts', dir: true },
  ],
  { level: 7, password: '' },                  // level 1 to 11 (default 7); password is optional
  (done, total) => console.log(done / total), // optional progress, after each block
);
```

- `type` is `"file"`, `"dir"`, or `"symlink"`. `mtime` is in Unix seconds. Read files in the order of `block` so that each block is decoded once; an archive object keeps the two most recently decoded blocks.
- Other methods: `isEncrypted()`, `isSigned()`, `signerKey()` (hex Ed25519 public key; the signature is checked when the archive opens), `digest()` (the archive fingerprint), `clearCache()`, and `version()`.
- Errors are thrown with a readable message, for example for a wrong password or damaged data.

## Using it on a website

1. Copy `pkg/` and `worker.js` to the site, and serve `.wasm` files as `application/wasm`.
2. Run ezpz in a worker (`worker.js` is ready to use) for anything bigger than a few MB.
3. Once loaded, nothing needs the network.

Please credit EZPZ File where your users can see it, as described below.

## Building

You need Rust with the `wasm32-unknown-unknown` target, a clang that can compile to wasm32 (zstd is C code; on macOS install Homebrew's `llvm`, because Apple's clang has no WebAssembly backend), `wasm-bindgen-cli` 0.2.129, and Python 3. Then run:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129
web/build.sh
```

## License

The same as the rest of the repository: the [MIT License](../LICENSE). If you use this in a product, website, or service, please credit EZPZ File somewhere your users can see it, such as an about page, the credits, or a footer:

    Powered by EZPZ File (https://ezpzfile.com)
