//! "Brain" codecs - predictive coding with online learning.
//! Codec id 3 (brain) runs the full set of models; codec id 4 (brain-fast)
//! runs a smaller set that is about 30% faster and a few percent larger.
//!
//! Idea in plain words:
//!   * Like the brain's *predictive coding*, we only spend bits on surprise:
//!     every bit is predicted first, and an arithmetic coder charges
//!     -log2(p) bits, so well-predicted bits cost almost nothing.
//!   * Many small "experts" (context models) each make a guess. Each expert
//!     remembers a short *history* per situation, and a learned table turns
//!     that history into a probability (like a memory that says "after this
//!     pattern, a 1 usually came next").
//!   * Small neural networks (mixers of artificial neurons) learn, bit by bit,
//!     whom to trust in which situation.
//!   * Encoder and decoder are identical twins: they see the same bits in the
//!     same order and learn in exactly the same way, so the learned model never
//!     has to be stored in the file.
//!
//! Everything is integer arithmetic so every machine produces identical bits.
//! The exact algorithm is normative - see SPEC.md §6.4 (brain) and §6.5 (brain-fast).

use anyhow::{Result, ensure};
use std::sync::OnceLock;

pub const MIN_TABLE_BITS: u8 = 16;
pub const MAX_TABLE_BITS: u8 = 25;

/// Which set of models runs. Each profile is its own codec id in the archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Codec 3: nine context models, three mixers, two APM stages.
    Full,
    /// Codec 4: the first six context models, two mixers, one APM stage.
    Fast,
}

// ------------------------------------------------------------ squash/stretch

const SQUASH_T: [i32; 33] = [
    1, 2, 3, 6, 10, 16, 27, 45, 73, 120, 194, 310, 488, 747, 1101, 1546, 2047, 2549, 2994, 3348,
    3607, 3785, 3901, 3975, 4022, 4050, 4068, 4079, 4085, 4089, 4092, 4093, 4094,
];

/// Logistic function: stretch domain (-2047..2047, 1/256 nats) -> 12-bit probability.
#[inline]
pub fn squash(d: i32) -> i32 {
    if d > 2047 {
        return 4095;
    }
    if d < -2047 {
        return 1;
    }
    let w = d & 127;
    let i = ((d >> 7) + 16) as usize;
    (SQUASH_T[i] * (128 - w) + SQUASH_T[i + 1] * w + 64) >> 7
}

fn stretch_table() -> &'static [i16; 4096] {
    static T: OnceLock<Box<[i16; 4096]>> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = Box::new([0i16; 4096]);
        let mut pi = 0usize;
        for x in -2047..=2047 {
            let v = squash(x) as usize;
            for item in t.iter_mut().take(v + 1).skip(pi) {
                *item = x as i16;
            }
            pi = pi.max(v + 1);
        }
        for item in t.iter_mut().skip(pi) {
            *item = 2047;
        }
        t
    })
}

#[inline]
fn hash2(a: u32, b: u32) -> u32 {
    let mut h = a
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(b.wrapping_mul(0x85EB_CA6B));
    h ^= h >> 16;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 13;
    h
}

// ------------------------------------------------------------ adaptive probabilities
// slot = ((p22 ^ 0x200000) << 10) | n  - so an all-zero slot means p = 1/2, n = 0.

static RECIP: OnceLock<[u32; 1024]> = OnceLock::new();
fn recip() -> &'static [u32; 1024] {
    RECIP.get_or_init(|| {
        let mut r = [0u32; 1024];
        for (n, v) in r.iter_mut().enumerate() {
            *v = 131_072 / (2 * n as u32 + 3); // 65536 / (n + 1.5)
        }
        r
    })
}

#[inline]
fn slot_p12(s: u32) -> i32 {
    (((s >> 10) ^ 0x20_0000) >> 10) as i32
}

#[inline]
fn slot_update(s: &mut u32, y: i32, limit: u32, rc: &[u32; 1024]) {
    let n = *s & 1023;
    let p = ((*s >> 10) ^ 0x20_0000) as i64;
    let target: i64 = if y != 0 { (1 << 22) - 1 } else { 0 };
    let p = p + (((target - p) * rc[n as usize] as i64) >> 16);
    let n = if n < limit { n + 1 } else { n };
    *s = (((p as u32) ^ 0x20_0000) << 10) | n;
}

const LIMIT_O0: u32 = 60;
const LIMIT_SM: u32 = 1023;
const LIMIT_MATCH: u32 = 1023;

// ------------------------------------------------------------ bit histories
// A 13-bit history state: n0 (6 bits), n1 (6 bits), last bit (1 bit).
// When one count grows, a large opposite count is cut down, so recent
// behaviour matters more than old behaviour (non-stationary memory).

const N_STATES: usize = 1 << 13;

#[inline]
fn next_state(s: u16, y: i32) -> u16 {
    let mut n0 = s & 63;
    let mut n1 = (s >> 6) & 63;
    if y != 0 {
        if n1 < 63 {
            n1 += 1;
        }
        if n0 > 2 {
            n0 = (n0 >> 1) + 1;
        }
    } else {
        if n0 < 63 {
            n0 += 1;
        }
        if n1 > 2 {
            n1 = (n1 >> 1) + 1;
        }
    }
    n0 | (n1 << 6) | ((y as u16) << 12)
}

#[inline]
fn state_total(s: u16) -> u16 {
    (s & 63) + ((s >> 6) & 63)
}

// ------------------------------------------------------------ hash table: 64-byte lines, 2 ways

#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct Line([u16; 32]);

struct HashTable {
    t: Vec<Line>,
    shift: u32,
}

impl HashTable {
    /// `bits` = log2(number of u16 slots).
    fn new(bits: u8) -> Self {
        let line_bits = bits as u32 - 5;
        HashTable {
            t: vec![Line([0; 32]); 1usize << line_bits],
            shift: 32 - line_bits,
        }
    }
    /// Returns slot index of the way for context hash `h` (way slot 0 = check,
    /// slots 1..15 = the 15 nodes of a nibble's binary tree).
    #[inline]
    fn find(&mut self, h: u32) -> usize {
        let li = (h >> self.shift) as usize;
        let chk = ((h.wrapping_mul(0x2545_F491) >> 16) as u16) | 1;
        let l = &mut self.t[li].0;
        if l[0] == chk {
            return li << 5;
        }
        if l[16] == chk {
            return (li << 5) | 16;
        }
        let w = if state_total(l[1]) < state_total(l[17]) {
            0
        } else {
            16
        };
        l[w..w + 16].fill(0);
        l[w] = chk;
        (li << 5) | w
    }
    #[inline]
    fn get(&self, idx: usize) -> u16 {
        unsafe { *self.t.get_unchecked(idx >> 5).0.get_unchecked(idx & 31) }
    }
    #[inline]
    fn set(&mut self, idx: usize, v: u16) {
        unsafe {
            *self
                .t
                .get_unchecked_mut(idx >> 5)
                .0
                .get_unchecked_mut(idx & 31) = v
        }
    }
}

// ------------------------------------------------------------ neurons (mixers)

/// Most hashed context models any profile uses (the full profile).
const MAX_HASHED: usize = 9;
/// Most mixer inputs: the hashed models + order-0, match, bias.
const MAX_NIN: usize = MAX_HASHED + 3;
const MIXER_LR: i32 = 16;

struct Mixer {
    w: Vec<[i32; MAX_NIN]>,
    sel: usize,
    pr: i32,
}

impl Mixer {
    fn new(sets: usize) -> Self {
        Mixer {
            w: vec![[1 << 14; MAX_NIN]; sets],
            sel: 0,
            pr: 2048,
        }
    }
    /// Uses the first `nin` inputs and weights only.
    #[inline(always)]
    fn mix(&mut self, x: &[i32; MAX_NIN], sel: usize, nin: usize) -> i32 {
        self.sel = sel;
        let w = &self.w[sel];
        let mut dot: i64 = 0;
        for i in 0..nin {
            dot += x[i] as i64 * w[i] as i64;
        }
        let st = (dot >> 16).clamp(-2047, 2047) as i32;
        self.pr = squash(st);
        st
    }
    #[inline(always)]
    fn update(&mut self, x: &[i32; MAX_NIN], y: i32, nin: usize) {
        let err = ((y << 12) - self.pr) * MIXER_LR;
        let w = &mut self.w[self.sel];
        for i in 0..nin {
            w[i] = w[i].wrapping_add((x[i] * err) >> 16);
        }
    }
}

// ------------------------------------------------------------ APM / SSE

struct Apm {
    t: Vec<u16>,
    idx: usize,
}

impl Apm {
    fn new(n: usize) -> Self {
        let mut t = vec![0u16; n * 33];
        for i in 0..n {
            for j in 0..33 {
                t[i * 33 + j] = (squash((j as i32 - 16) * 128) * 16) as u16;
            }
        }
        Apm { t, idx: 0 }
    }
    #[inline]
    fn pp(&mut self, st: i32, cx: usize) -> i32 {
        let s = st + 2048;
        let lo = (s >> 7) as usize;
        let w = s & 127;
        let base = cx * 33 + lo;
        self.idx = base + (w >> 6) as usize;
        (self.t[base] as i32 * (128 - w) + self.t[base + 1] as i32 * w) >> 11
    }
    #[inline]
    fn update(&mut self, y: i32) {
        const RATE: i32 = 7;
        let g = (y << 16) + (y << RATE) - y - y;
        let t = self.t[self.idx] as i32;
        self.t[self.idx] = (t + ((g - t) >> RATE)) as u16;
    }
}

// ------------------------------------------------------------ predictor

const MATCH_MIN: usize = 6;
const MATCH_KEEP: usize = 16;

/// `FAST = false` is codec 3 (brain), `FAST = true` is codec 4 (brain-fast).
pub struct Predictor<const FAST: bool> {
    st: &'static [i16; 4096],
    rc: &'static [u32; 1024],
    t0: Vec<u32>,
    tables: Vec<HashTable>,
    sm: Vec<Vec<u32>>,
    ctx: [u32; MAX_HASHED],
    base: [usize; MAX_HASHED],
    state: [u16; MAX_HASHED],
    // match model
    hist: Vec<u8>,
    mt: Vec<u32>,
    mt_shift: u32,
    m_ptr: usize,
    m_len: usize,
    m_sm: [u32; 64],
    m_ctx: usize, // 0 = no prediction this bit, else 1 + index
    m_lq: usize,
    mix: Vec<Mixer>,
    apm1: Apm,
    apm2: Apm, // empty in the fast profile
    x: [i32; MAX_NIN],
    c0: u32,
    nib: u32,
    bitpos: u32,
    c4: u32,
    c8: u32,
    word: u32,
    pword: u32,
    pr: i32,
}

impl<const FAST: bool> Predictor<FAST> {
    /// Hashed context models: 0..=8 in the full profile, 0..=5 in the fast one.
    const N_HASHED: usize = if FAST { 6 } else { 9 };
    /// Mixer inputs: the hashed models, then order-0, match, bias.
    const NIN: usize = Self::N_HASHED + 3;
    const N_MIX: usize = if FAST { 2 } else { 3 };

    pub fn new(table_bits: u8, expected_len: usize) -> Self {
        let tb = table_bits;
        let mut tables = Vec::with_capacity(Self::N_HASHED);
        for i in 0..Self::N_HASHED {
            let bits = if i == 0 { tb.min(18) } else { tb };
            tables.push(HashTable::new(bits));
        }
        let mbits = (tb - 3) as u32;
        let mut p = Predictor {
            st: stretch_table(),
            rc: recip(),
            t0: vec![0u32; 256],
            tables,
            sm: vec![vec![0u32; N_STATES]; Self::N_HASHED],
            ctx: [0; MAX_HASHED],
            base: [0; MAX_HASHED],
            state: [0; MAX_HASHED],
            hist: Vec::with_capacity(expected_len),
            mt: vec![0u32; 1usize << mbits],
            mt_shift: 32 - mbits,
            m_ptr: 0,
            m_len: 0,
            m_sm: [0; 64],
            m_ctx: 0,
            m_lq: 0,
            mix: (0..Self::N_MIX).map(|_| Mixer::new(256)).collect(),
            apm1: Apm::new(256),
            apm2: Apm::new(if FAST { 0 } else { 65536 }),
            x: [0; MAX_NIN],
            c0: 1,
            nib: 1,
            bitpos: 0,
            c4: 0,
            c8: 0,
            word: 0,
            pword: 0,
            pr: 2048,
        };
        p.byte_contexts();
        p.select_buckets();
        p
    }

    #[inline]
    fn stretch(&self, p: i32) -> i32 {
        self.st[p as usize] as i32
    }

    fn byte_contexts(&mut self) {
        let c4 = self.c4;
        let c1 = c4 & 0xFF;
        self.ctx[0] = c1;
        self.ctx[1] = c4 & 0xFFFF;
        self.ctx[2] = c4 & 0xFF_FFFF;
        self.ctx[3] = c4;
        self.ctx[4] = hash2(c4, self.c8 & 0xFFFF);
        self.ctx[5] = if self.word == 0 {
            hash2(c1, 0x5757)
        } else {
            self.word
        };
        if !FAST {
            self.ctx[6] = hash2(self.word, self.pword.wrapping_add(0x3131));
            self.ctx[7] = c4 & 0x00FF_FF00;
            self.ctx[8] = c4 & 0xFF00_00FF;
        }
    }

    fn select_buckets(&mut self) {
        // Compute all hashes and touch all lines first so the memory loads overlap.
        let mut hs = [0u32; MAX_HASHED];
        let mut touch = 0u16;
        for i in 0..Self::N_HASHED {
            let h = hash2(self.ctx[i].wrapping_add((i as u32) << 28), self.c0);
            hs[i] = h;
            let t = &self.tables[i];
            touch ^= t.get(((h >> t.shift) as usize) << 5);
        }
        std::hint::black_box(touch);
        for i in 0..Self::N_HASHED {
            self.base[i] = self.tables[i].find(hs[i]);
        }
    }

    fn match_byte_update(&mut self) {
        let pos = self.hist.len();
        let last = self.hist[pos - 1];
        if self.m_len > 0 {
            if self.hist[self.m_ptr] == last {
                self.m_len = (self.m_len + 1).min(65535);
            } else if self.m_len >= MATCH_KEEP {
                self.m_len = 1; // a long match broke: keep following it, with low confidence
            } else {
                self.m_len = 0;
            }
            if self.m_len > 0 {
                self.m_ptr += 1;
            }
        }
        if pos >= MATCH_MIN {
            let h = &self.hist[pos - MATCH_MIN..pos];
            let a = u32::from_le_bytes([h[2], h[3], h[4], h[5]]);
            let b = u32::from_le_bytes([h[0], h[1], 0, 0]);
            let hh = hash2(a, b.wrapping_add(0x6d6d_6d6d));
            let slot = (hh >> self.mt_shift) as usize;
            if self.m_len < MATCH_KEEP {
                let cand = self.mt[slot] as usize;
                if cand > 0 {
                    let mut l = 0usize;
                    while l < 32 && l < cand && self.hist[cand - 1 - l] == self.hist[pos - 1 - l] {
                        l += 1;
                    }
                    if l >= MATCH_MIN && l > self.m_len {
                        self.m_len = l;
                        self.m_ptr = cand;
                    }
                }
            }
            self.mt[slot] = pos as u32;
        }
        let l = self.m_len;
        self.m_lq = if l < 16 {
            l
        } else {
            (16 + ((l - 16) >> 2)).min(31)
        };
    }

    /// Probability (1..4095, of 4096) that the next bit is 1.
    #[inline]
    pub fn p(&mut self) -> i32 {
        let nh = Self::N_HASHED;
        let nib = self.nib as usize;
        for i in 0..nh {
            let s = self.tables[i].get(self.base[i] + nib) & (N_STATES as u16 - 1);
            self.state[i] = s;
            let v = unsafe { *self.sm.get_unchecked(i).get_unchecked(s as usize) };
            self.x[i] = self.stretch(slot_p12(v));
        }
        self.x[nh] = self.stretch(slot_p12(self.t0[self.c0 as usize]));
        self.m_ctx = 0;
        self.x[nh + 1] = 0;
        if self.m_len > 0 {
            let pb = self.hist[self.m_ptr] as u32 | 0x100;
            let shift = 8 - self.bitpos;
            if (pb >> shift) == self.c0 {
                let bit = ((pb >> (shift - 1)) & 1) as usize;
                let cx = self.m_lq * 2 + bit;
                self.m_ctx = 1 + cx;
                self.x[nh + 1] = self.stretch(slot_p12(self.m_sm[cx]));
            }
        }
        self.x[nh + 2] = 256;

        let c0 = self.c0 as usize;
        let c1 = (self.c4 & 0xFF) as usize;
        let st = if FAST {
            // Mixers A (by bit position in the byte) and C (by previous byte).
            let sum = self.mix[0].mix(&self.x, c0, Self::NIN) + self.mix[1].mix(&self.x, c1, Self::NIN);
            sum >> 1 // average of 2
        } else {
            let lq = if self.m_ctx == 0 { 0 } else { self.m_lq };
            let sels = [c0, lq * 8 + self.bitpos as usize, c1];
            let mut sum = 0i32;
            for k in 0..3 {
                sum += self.mix[k].mix(&self.x, sels[k], Self::NIN);
            }
            (sum * 21846) >> 16 // average of 3
        };
        let pm = squash(st);
        let a1 = self.apm1.pp(st, c0);
        let a2 = if FAST { a1 } else { self.apm2.pp(st, c0 | (c1 << 8)) };
        self.pr = ((pm + a1 + 2 * a2 + 2) >> 2).clamp(1, 4095);
        self.pr
    }

    #[inline]
    pub fn update(&mut self, y: i32) {
        let rc = self.rc;
        let nib = self.nib as usize;
        for i in 0..Self::N_HASHED {
            let s = self.state[i];
            let sm = unsafe { self.sm.get_unchecked_mut(i).get_unchecked_mut(s as usize) };
            slot_update(sm, y, LIMIT_SM, rc);
            let idx = self.base[i] + nib;
            let full = self.tables[i].get(idx);
            self.tables[i].set(idx, next_state(full, y));
        }
        slot_update(&mut self.t0[self.c0 as usize], y, LIMIT_O0, rc);
        if self.m_ctx != 0 {
            slot_update(&mut self.m_sm[self.m_ctx - 1], y, LIMIT_MATCH, rc);
        }
        for k in 0..Self::N_MIX {
            self.mix[k].update(&self.x, y, Self::NIN);
        }
        self.apm1.update(y);
        if !FAST {
            self.apm2.update(y);
        }

        self.c0 = (self.c0 << 1) | y as u32;
        self.nib = (self.nib << 1) | y as u32;
        self.bitpos += 1;
        if self.bitpos == 8 {
            let c = (self.c0 & 0xFF) as u8;
            self.hist.push(c);
            self.c8 = (self.c8 << 8) | (self.c4 >> 24);
            self.c4 = (self.c4 << 8) | c as u32;
            let lc = c.to_ascii_lowercase();
            if lc.is_ascii_lowercase() || c >= 0x80 {
                self.word = hash2(self.word.wrapping_add(lc as u32), 0x7777_0001);
            } else if self.word != 0 {
                self.pword = self.word;
                self.word = 0;
            }
            self.match_byte_update();
            self.c0 = 1;
            self.nib = 1;
            self.bitpos = 0;
            self.byte_contexts();
            self.select_buckets();
        } else if self.bitpos == 4 {
            self.nib = 1;
            self.select_buckets();
        }
    }
}

// ------------------------------------------------------------ arithmetic coder

struct Encoder {
    x1: u32,
    x2: u32,
    out: Vec<u8>,
}

impl Encoder {
    #[inline]
    fn encode(&mut self, bit: i32, p: i32) {
        let r = self.x2 - self.x1;
        let xmid = self.x1 + (r >> 12) * p as u32 + (((r & 0xFFF) * p as u32) >> 12);
        if bit != 0 {
            self.x2 = xmid;
        } else {
            self.x1 = xmid + 1;
        }
        while (self.x1 ^ self.x2) & 0xFF00_0000 == 0 {
            self.out.push((self.x2 >> 24) as u8);
            self.x1 <<= 8;
            self.x2 = (self.x2 << 8) | 255;
        }
    }
    fn finish(mut self) -> Vec<u8> {
        self.out.extend_from_slice(&self.x1.to_be_bytes());
        self.out
    }
}

struct Decoder<'a> {
    x1: u32,
    x2: u32,
    x: u32,
    src: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    fn new(src: &'a [u8]) -> Self {
        let mut d = Decoder {
            x1: 0,
            x2: u32::MAX,
            x: 0,
            src,
            pos: 0,
        };
        for _ in 0..4 {
            d.x = (d.x << 8) | d.next() as u32;
        }
        d
    }
    #[inline]
    fn next(&mut self) -> u8 {
        let b = self.src.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        b
    }
    #[inline]
    fn decode(&mut self, p: i32) -> i32 {
        let r = self.x2 - self.x1;
        let xmid = self.x1 + (r >> 12) * p as u32 + (((r & 0xFFF) * p as u32) >> 12);
        let bit = if self.x <= xmid {
            self.x2 = xmid;
            1
        } else {
            self.x1 = xmid + 1;
            0
        };
        while (self.x1 ^ self.x2) & 0xFF00_0000 == 0 {
            self.x1 <<= 8;
            self.x2 = (self.x2 << 8) | 255;
            self.x = (self.x << 8) | self.next() as u32;
        }
        bit
    }
}

// ------------------------------------------------------------ public API

/// Table size the encoder picks for a block of `n` bytes (decoders just read it).
pub fn table_bits_for(n: usize) -> u8 {
    let mut b = 0u8;
    while (1usize << b) < n && b < 40 {
        b += 1;
    }
    (b + 1).clamp(MIN_TABLE_BITS, 24)
}

pub fn compress(raw: &[u8], profile: Profile) -> Vec<u8> {
    match profile {
        Profile::Full => compress_with::<false>(raw),
        Profile::Fast => compress_with::<true>(raw),
    }
}

pub fn decompress(stored: &[u8], raw_len: usize, profile: Profile) -> Result<Vec<u8>> {
    match profile {
        Profile::Full => decompress_with::<false>(stored, raw_len),
        Profile::Fast => decompress_with::<true>(stored, raw_len),
    }
}

fn compress_with<const FAST: bool>(raw: &[u8]) -> Vec<u8> {
    let tb = table_bits_for(raw.len());
    let mut out = Vec::with_capacity(raw.len() / 3 + 16);
    out.push(tb);
    let mut p = Predictor::<FAST>::new(tb, raw.len());
    let mut e = Encoder {
        x1: 0,
        x2: u32::MAX,
        out,
    };
    for &c in raw {
        for i in (0..8).rev() {
            let bit = ((c >> i) & 1) as i32;
            e.encode(bit, p.p());
            p.update(bit);
        }
    }
    e.finish()
}

fn decompress_with<const FAST: bool>(stored: &[u8], raw_len: usize) -> Result<Vec<u8>> {
    ensure!(!stored.is_empty(), "empty brain block");
    let tb = stored[0];
    ensure!(
        (MIN_TABLE_BITS..=MAX_TABLE_BITS).contains(&tb),
        "brain table size out of range"
    );
    let mut p = Predictor::<FAST>::new(tb, raw_len);
    let mut d = Decoder::new(&stored[1..]);
    let mut out = Vec::with_capacity(raw_len);
    for _ in 0..raw_len {
        let mut c = 0u32;
        for _ in 0..8 {
            let bit = d.decode(p.p());
            p.update(bit);
            c = (c << 1) | bit as u32;
        }
        out.push(c as u8);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let mut data = Vec::new();
        for i in 0..20000u32 {
            data.extend_from_slice(
                format!("line {} says hello world {}\n", i % 97, i * 7).as_bytes(),
            );
        }
        data.extend((0..5000u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8));
        for profile in [Profile::Full, Profile::Fast] {
            let c = compress(&data, profile);
            assert!(c.len() < data.len() / 4);
            assert_eq!(decompress(&c, data.len(), profile).unwrap(), data);
            for n in [0usize, 1, 2, 7, 100] {
                let d: Vec<u8> = (0..n as u8).collect();
                assert_eq!(decompress(&compress(&d, profile), n, profile).unwrap(), d);
            }
        }
        // The two profiles are different codecs: they must not decode each other's output.
        let c = compress(&data, Profile::Fast);
        assert_ne!(decompress(&c, data.len(), Profile::Full).unwrap(), data);
    }
}
