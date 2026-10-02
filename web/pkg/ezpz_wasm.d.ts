/* tslint:disable */
/* eslint-disable */

/**
 * An opened archive.
 */
export class EzpzArchive {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Frees the decoded blocks kept between `read` calls.
     */
    clearCache(): void;
    /**
     * Archive fingerprint (root hash), in hex.
     */
    digest(): string;
    /**
     * JSON array of entries, sorted by path:
     * `[{"path", "type": "file"|"dir"|"symlink", "size", "mode", "mtime", "block", "target"}]`.
     * `mtime` is in Unix seconds. `block` is the first block a file uses: reading files in
     * that order decodes each block once.
     */
    entriesJson(): string;
    /**
     * JSON summary: `{"encrypted", "signed", "blocks": {codec name: count}, "storedBytes",
     * "files", "dirs", "links", "contentBytes", "creator", "created"}`. Without the password
     * of an encrypted archive, only the first four are filled in.
     */
    infoJson(): string;
    isEncrypted(): boolean;
    isSigned(): boolean;
    /**
     * True for an encrypted archive opened without a password.
     */
    needsPassword(): boolean;
    /**
     * Opens archive bytes and checks the header, index, and signature. An encrypted archive
     * needs `password`; without it the archive opens for checking only (`needsPassword()`).
     * A wrong password throws.
     */
    constructor(bytes: Uint8Array, password?: string | null);
    /**
     * Content of one file (hash checked). Throws if `path` is not a file in the archive.
     */
    read(path: string): Uint8Array;
    /**
     * Ed25519 public key of a signed archive, in hex (the signature was already checked).
     */
    signerKey(): string | undefined;
    /**
     * Checks every block hash and, if the archive could be opened fully, every file hash.
     * Returns JSON: `{"ok", "blocksOk", "blocksBad": [...], "filesOk", "filesBad": [...],
     * "contentChecked"}`.
     */
    verifyJson(): string;
}

/**
 * Creates an archive and returns its bytes.
 *
 * `files`: array of `{path: string, data?: Uint8Array, dir?: boolean, target?: string,
 * mtime?: number (milliseconds, like Date.now()), mode?: number}`. Paths use '/' and may
 * include folders ("photos/2026/a.jpg"); folders that are not listed are added.
 *
 * `options`: `{level?: 1..11 (default 7), password?: string, dedup?: boolean,
 * filters?: boolean, hashLen?: 0|16|32}`. Levels 10 (`--max`) and 11 use the brain codecs,
 * which are slow in a browser too.
 *
 * `onProgress(done, total)`: called after each block with input bytes.
 */
export function create(files: Array<any>, options: any, on_progress?: Function | null): Uint8Array;

/**
 * Version of the ezpz library inside this build.
 */
export function version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_ezpzarchive_free: (a: number, b: number) => void;
    readonly create: (a: any, b: any, c: number) => [number, number, number, number];
    readonly ezpzarchive_clearCache: (a: number) => void;
    readonly ezpzarchive_digest: (a: number) => [number, number];
    readonly ezpzarchive_entriesJson: (a: number) => [number, number, number, number];
    readonly ezpzarchive_infoJson: (a: number) => [number, number];
    readonly ezpzarchive_isEncrypted: (a: number) => number;
    readonly ezpzarchive_isSigned: (a: number) => number;
    readonly ezpzarchive_needsPassword: (a: number) => number;
    readonly ezpzarchive_new: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly ezpzarchive_read: (a: number, b: number, c: number) => [number, number, number, number];
    readonly ezpzarchive_signerKey: (a: number) => [number, number];
    readonly ezpzarchive_verifyJson: (a: number) => [number, number];
    readonly version: () => [number, number];
    readonly rust_zstd_wasm_shim_calloc: (a: number, b: number) => number;
    readonly rust_zstd_wasm_shim_free: (a: number) => void;
    readonly rust_zstd_wasm_shim_malloc: (a: number) => number;
    readonly rust_zstd_wasm_shim_memcmp: (a: number, b: number, c: number) => number;
    readonly rust_zstd_wasm_shim_memcpy: (a: number, b: number, c: number) => number;
    readonly rust_zstd_wasm_shim_memmove: (a: number, b: number, c: number) => number;
    readonly rust_zstd_wasm_shim_memset: (a: number, b: number, c: number) => number;
    readonly rust_zstd_wasm_shim_qsort: (a: number, b: number, c: number, d: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
