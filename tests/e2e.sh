#!/usr/bin/env bash
# End-to-end tests for the ezpz CLI. Usage: tests/e2e.sh [path/to/ezpz]
set -u
B=${1:-$(dirname "$0")/../target/release/ezpz}
B=$(realpath "$B")
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
cd "$W"
pass=0; fail=0
ok()   { echo "  PASS  $1"; pass=$((pass+1)); }
bad()  { echo "  FAIL  $1"; fail=$((fail+1)); }
expect_ok()   { local n=$1; shift; if "$@" >/dev/null 2>&1; then ok "$n"; else bad "$n"; fi; }
expect_fail() { local n=$1; shift; if "$@" >/dev/null 2>&1; then bad "$n (should fail)"; else ok "$n"; fi; }

# --- test tree: text, binary, duplicates, empty file, empty dir, unicode name, symlinks
# (the Korean folder/file names and text are deliberate: they exercise non-ASCII UTF-8 paths)
mkdir -p src/deep/er src/emptydir src/한글폴더
for i in $(seq 1 300); do echo "line $i: the quick brown fox jumps over the lazy dog $((i*i))"; done > src/text.txt
head -c 300000 /dev/urandom > src/random.bin
cp src/text.txt src/deep/er/copy.txt
cp "$(command -v ls)" src/prog
: > src/empty
echo "안녕하세요" > src/한글폴더/인사.txt
ln -s ../text.txt src/deep/link
ln -s /etc/passwd src/deep/escape
head -c 3000000 /dev/urandom > src/big.bin; cat src/big.bin src/big.bin > src/big2x.bin   # dedup across >2MB

echo "[roundtrip]"
for lv in 1 6 9 10 11; do
  rm -rf out; "$B" c a$lv.ezpz src -l $lv >/dev/null 2>&1 && "$B" x a$lv.ezpz -C out >/dev/null 2>&1 \
    && diff -r --no-dereference -x escape src out/src >/dev/null && ok "level $lv" || bad "level $lv"
done
rm -rf out; "$B" c nd.ezpz src --no-dedup --no-filter >/dev/null 2>&1 && "$B" x nd.ezpz -C out >/dev/null 2>&1 \
  && diff -r -x escape src out/src >/dev/null && ok "no-dedup/no-filter" || bad "no-dedup/no-filter"
for c in zstd lzma2 brain brain-fast store; do
  rm -rf out; "$B" c c_$c.ezpz src --codec $c >/dev/null 2>&1 && "$B" x c_$c.ezpz -C out >/dev/null 2>&1 \
    && diff -r -x escape src out/src >/dev/null && ok "codec $c" || bad "codec $c"
done
rm -rf out; "$B" c m.ezpz src --codec brain-fast --brain-mask 0x155 >/dev/null 2>&1 && "$B" x m.ezpz -C out >/dev/null 2>&1 \
  && diff -r -x escape src out/src >/dev/null && ok "brain-fast with an unusual model mask" || bad "brain-fast with an unusual model mask"
s1=$(stat -c %s a6.ezpz); [ "$s1" -lt 4000000 ] && ok "dedup stores repeated 3MB once ($s1 bytes)" || bad "dedup ($s1 bytes)"

echo "[random access]"
[ "$("$B" cat a6.ezpz src/한글폴더/인사.txt)" = "안녕하세요" ] && ok "cat unicode path" || bad "cat unicode path"
rm -rf out; "$B" x a6.ezpz src/deep -C out >/dev/null 2>&1 && [ -f out/src/deep/er/copy.txt ] && [ ! -e out/src/text.txt ] \
  && ok "partial extract" || bad "partial extract"

echo "[safety]"
rm -rf out; "$B" x a6.ezpz -C out >/dev/null 2>&1; [ ! -e out/src/deep/escape ] && ok "escaping symlink not created" || bad "escaping symlink created"
[ -L out/src/deep/link ] && ok "safe symlink created" || bad "safe symlink missing"
expect_fail "refuse overwrite without --force" "$B" x a6.ezpz -C out
expect_ok   "overwrite with --force" "$B" x a6.ezpz -C out --force

echo "[integrity]"
expect_ok "verify clean" "$B" verify a6.ezpz
sz=$(stat -c %s a6.ezpz)
for off in 40 1000 $((sz/2)) $((sz-150)) $((sz-70)) $((sz-10)) 9; do
  cp a6.ezpz t.ezpz; printf '\x5a' | dd of=t.ezpz bs=1 seek=$off conv=notrunc 2>/dev/null
  if cmp -s a6.ezpz t.ezpz; then printf '\xa5' | dd of=t.ezpz bs=1 seek=$off conv=notrunc 2>/dev/null; fi
  expect_fail "detect 1-byte change at offset $off" "$B" verify t.ezpz
done
head -c $((sz-1)) a6.ezpz > t.ezpz; expect_fail "detect truncation" "$B" verify t.ezpz
cp a6.ezpz t.ezpz; echo junk >> t.ezpz; expect_fail "detect appended junk" "$B" verify t.ezpz
expect_fail "reject non-archive" "$B" list src/text.txt

echo "[encryption]"
expect_ok   "create encrypted" "$B" c e.ezpz src --password 's3cret!'
expect_fail "wrong password" "$B" list e.ezpz --password nope
expect_ok   "right password verify" "$B" verify e.ezpz --password 's3cret!'
expect_ok   "structural verify w/o password" "$B" verify e.ezpz --no-password
"$B" list a6.ezpz >/dev/null; if grep -q "인사" e.ezpz 2>/dev/null; then bad "names hidden"; else ok "names hidden"; fi
rm -rf out; EZPZ_PASSWORD='s3cret!' "$B" x e.ezpz -C out >/dev/null 2>&1 && diff -r -x escape src out/src >/dev/null \
  && ok "decrypt roundtrip (env password)" || bad "decrypt roundtrip"
cp e.ezpz t.ezpz; esz=$(stat -c %s e.ezpz); printf '\x00' | dd of=t.ezpz bs=1 seek=200 conv=notrunc 2>/dev/null
expect_fail "encrypted tamper caught w/o password" "$B" verify t.ezpz --no-password

echo "[signatures]"
"$B" keygen alice >/dev/null; "$B" keygen mallory >/dev/null
expect_ok   "create signed" "$B" c s.ezpz src --sign alice.key
expect_ok   "verify with right key" "$B" verify s.ezpz --pubkey alice.pub
expect_fail "verify with wrong key" "$B" verify s.ezpz --pubkey mallory.pub
expect_fail "unsigned archive with --pubkey" "$B" verify a6.ezpz --pubkey alice.pub
cp s.ezpz t.ezpz; ssz=$(stat -c %s s.ezpz); printf '\x00' | dd of=t.ezpz bs=1 seek=$((ssz-64-30)) conv=notrunc 2>/dev/null
expect_fail "corrupted signature rejected" "$B" list t.ezpz
expect_ok   "signed + encrypted" "$B" c se.ezpz src --sign alice.key --password pw
expect_ok   "verify signed+encrypted without password" "$B" verify se.ezpz --no-password --pubkey alice.pub

echo
echo "passed: $pass  failed: $fail"
[ "$fail" -eq 0 ]
