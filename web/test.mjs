// Tests the WebAssembly build in Node (18 or newer): node web/test.mjs
// Reads every archive in testvectors/ and compares the files with the inputs, then creates
// archives at several levels (with and without a password) and reads them back.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as ez from './pkg/ezpz_wasm.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const tv = path.join(here, '..', 'testvectors');
ez.initSync({ module: fs.readFileSync(path.join(here, 'pkg', 'ezpz_wasm_bg.wasm')) });

let pass = 0, fail = 0;
const check = (name, ok, extra = '') => {
  ok ? pass++ : fail++;
  console.log(`  ${ok ? 'PASS' : 'FAIL'}  ${name}${extra ? ' ' + extra : ''}`);
};

function readAll(archive, root) {
  const entries = JSON.parse(archive.entriesJson()).filter((e) => e.type === 'file');
  entries.sort((a, b) => a.block - b.block);
  for (const e of entries) {
    const got = Buffer.from(archive.read(e.path));
    if (!got.equals(fs.readFileSync(path.join(root, e.path)))) return `differs: ${e.path}`;
  }
  return entries.length;
}

console.log(`[ezpz ${ez.version()} WebAssembly: test vectors]`);
if (!fs.existsSync(path.join(tv, 'input-x64big'))) {
  console.log('  (run "python3 testvectors/make_big64.py" first to include x86-64-split-big.ezpz)');
}
for (const name of fs.readdirSync(tv).filter((f) => f.endsWith('.ezpz')).sort()) {
  if (name === 'x86-64-split-big.ezpz' && !fs.existsSync(path.join(tv, 'input-x64big'))) continue;
  try {
    const a = new ez.EzpzArchive(fs.readFileSync(path.join(tv, name)));
    const n = readAll(a, tv);
    const v = JSON.parse(a.verifyJson());
    check(name, typeof n === 'number' && v.ok, typeof n === 'number' ? `(${n} files)` : n);
    a.free();
  } catch (e) {
    check(name, false, e.message);
  }
}
try {
  const a = new ez.EzpzArchive(fs.readFileSync(path.join(tv, 'signed.ezpz')));
  check('signed.ezpz: public key matches k.pub', a.signerKey() === fs.readFileSync(path.join(tv, 'k.pub'), 'utf8').trim());
} catch (e) {
  check('signed.ezpz key', false, e.message);
}

console.log('[create and read back]');
const files = [];
(function walk(dir, rel) {
  for (const n of fs.readdirSync(dir).sort()) {
    const p = path.join(dir, n), r = rel + '/' + n, st = fs.lstatSync(p);
    if (st.isSymbolicLink()) files.push({ path: r, target: fs.readlinkSync(p) });
    else if (st.isDirectory()) { files.push({ path: r, dir: true }); walk(p, r); }
    else files.push({ path: r, data: fs.readFileSync(p), mtime: st.mtimeMs });
  }
})(path.join(tv, 'input'), 'input');
files.push({ path: 'input-x64/prog64', data: fs.readFileSync(path.join(tv, 'input-x64', 'prog64')) });
for (const [level, password] of [[1], [7], [9], [10], [11], [7, '비밀 password']]) {
  const name = `level ${level}${password ? ' with a password' : ''}`;
  try {
    const bytes = ez.create(files, { level, password }, () => {});
    if (password) {
      const locked = new ez.EzpzArchive(bytes);
      if (!locked.needsPassword()) throw new Error('opened without the password');
      try { new ez.EzpzArchive(bytes, 'wrong'); throw new Error('wrong password accepted'); } catch (e) { if (/accepted/.test(e.message)) throw e; }
    }
    const a = new ez.EzpzArchive(bytes, password);
    const n = readAll(a, tv);
    check(name, typeof n === 'number' && JSON.parse(a.verifyJson()).ok, `(${bytes.length} bytes)`);
  } catch (e) {
    check(name, false, e.message);
  }
}
console.log(`passed: ${pass}  failed: ${fail}`);
process.exit(fail ? 1 : 0);
