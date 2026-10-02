// ezpz Web Worker: runs the WebAssembly build off the main thread, so a page stays responsive
// while a large archive is being compressed or extracted. Messages in and out:
//
//   -> {id, op: 'init', wasm: ArrayBuffer}           <- {id, ok: true, version}
//   -> {id, op: 'open', bytes: ArrayBuffer, password} <- {id, ok, needsPassword, encrypted, signed,
//                                                          signerKey, digest, info, entries}
//   -> {id, op: 'read', paths: [string]}              <- {id, ok, files: [{path, data: ArrayBuffer}]}
//   -> {id, op: 'verify'}                             <- {id, ok, report}
//   -> {id, op: 'create', files, options}             <- {id, ok, bytes: ArrayBuffer}
//                                                     <- {id, progress: [done, total]} while working
//   -> {id, op: 'close'}                              <- {id, ok}
//
// Errors come back as {id, ok: false, error: message}. `files` for 'create' is an array of
// {path, data?: ArrayBuffer | Uint8Array, dir?: true, mtime?: ms, mode?: number}.
//
// As a module worker it imports the wasm-bindgen glue below. The single-file page
// (web/dist/ezpz.html) runs it as a classic worker instead, with `ez` set to the global of the
// no-modules glue, because browsers refuse module workers on pages opened from the disk.
import * as ez from './pkg/ezpz_wasm.js';

let archive = null;

function transferList(files) {
  return files.map((f) => f.data);
}

self.onmessage = (ev) => {
  const m = ev.data;
  const reply = (msg, transfer) => self.postMessage({ id: m.id, ...msg }, transfer || []);
  try {
    switch (m.op) {
      case 'init': {
        ez.initSync({ module: m.wasm });
        reply({ ok: true, version: ez.version() });
        break;
      }
      case 'open': {
        if (archive) archive.free();
        archive = null; // a failed open (wrong password) must not leave a freed object behind
        archive = new ez.EzpzArchive(new Uint8Array(m.bytes), m.password || undefined);
        const needs = archive.needsPassword();
        reply({
          ok: true,
          needsPassword: needs,
          encrypted: archive.isEncrypted(),
          signed: archive.isSigned(),
          signerKey: archive.signerKey() || null,
          digest: archive.digest(),
          info: JSON.parse(archive.infoJson()),
          entries: needs ? [] : JSON.parse(archive.entriesJson()),
        });
        break;
      }
      case 'read': {
        if (!archive) throw new Error('no archive is open');
        const files = m.paths.map((p) => ({ path: p, data: archive.read(p).buffer }));
        reply({ ok: true, files }, transferList(files));
        break;
      }
      case 'verify': {
        if (!archive) throw new Error('no archive is open');
        reply({ ok: true, report: JSON.parse(archive.verifyJson()) });
        break;
      }
      case 'create': {
        const files = m.files.map((f) => ({
          ...f,
          data: f.data ? new Uint8Array(f.data) : undefined,
        }));
        const out = ez.create(files, m.options || {}, (done, total) => {
          self.postMessage({ id: m.id, progress: [done, total] });
        });
        reply({ ok: true, bytes: out.buffer }, [out.buffer]);
        break;
      }
      case 'close': {
        if (archive) archive.free();
        archive = null;
        reply({ ok: true });
        break;
      }
      default:
        throw new Error('unknown op ' + m.op);
    }
  } catch (e) {
    reply({ ok: false, error: e && e.message ? e.message : String(e) });
  }
};
