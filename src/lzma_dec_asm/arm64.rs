//! 7-Zip's hand-written arm64 LZMA decoder and the state it operates on.
//!
//! `src/asm/lzma_dec_opt_arm64.s` is a preprocessed copy of 7-Zip's
//! `Asm/arm64/LzmaDecOpt.S` (Igor Pavlov, public domain; see `src/asm/README.md`
//! for provenance and the regeneration script). It implements
//! `LzmaDec_DecodeReal_3()`, the symbol decoding loop of 7-Zip's C decoder, and
//! is tightly coupled to that decoder's data model: the [`CLzmaDec`] struct
//! layout, the flat probability array layout and the calling contract below are
//! all dictated by the assembly and must not change.
//!
//! [`AsmCore`] drives the routine the way `LzmaDec_DecodeToDic` and
//! `Lzma2Dec_DecodeToDic` do, minus the parts a reader that always holds a
//! whole chunk in memory does not need: there is no `tempBuf` and no
//! `LzmaDec_TryDummy`. Instead every chunk is followed by [`CHUNK_PAD`] zero
//! bytes, and a call that read past the chunk is reported as corrupt data.
//!
//! # Contract of the assembly function
//!
//! `decode_real(p, limit, buf_limit) -> 0 (ok) | 1 (data error)`
//!
//! In:
//! - the range coder is normalized and `remain_len == 0` (pending match bytes
//!   were copied by `write_rem`);
//! - `dic_pos < limit <= dic_buf_size`; when `check_dic_size == 0` the caller
//!   clamped `limit` so that `processed_pos` cannot pass `dic_size`
//!   (`LzmaDec_DecodeReal2`);
//! - `probs` holds `1984 + (0x300 << (lc + lp))` entries, and every
//!   `reps[i]` is in `1..=dic_buf_size` (they are validated back-references
//!   plus one);
//! - `buf <= buf_limit`, and the 20 bytes past `buf_limit` are readable: the
//!   loop decodes one symbol unconditionally and keeps going while
//!   `buf < buf_limit && dic_pos < limit`, so it may start a symbol at
//!   `buf_limit - 1` and read up to `LZMA_REQUIRED_INPUT_MAX` bytes for it.
//!
//! Out: `dic_pos`, `buf`, `range`, `code`, `processed_pos`, `reps`, `state`
//! and `remain_len` are written back; `check_dic_size` is never written.
//! `remain_len < 274` is the number of match bytes still to copy (the output
//! limit interrupted a match), `274` means an end marker was decoded (an error
//! inside LZMA2), `>= 512` accompanies a data error.
//!
//! The assembly saves x19..x30 on its own 128-byte frame, never touches x18,
//! uses no globals and calls nothing, so it is reentrant and thread-safe.

use alloc::vec::Vec;
use core::{
    arch::{asm, global_asm},
    ptr,
};

use crate::{
    DICT_SIZE_MIN, Read, error_invalid_data, error_invalid_input, error_out_of_memory,
    lzma2_reader::COMPRESSED_SIZE_MAX, lzma_reader::RC_INIT_SIZE,
};

/// Offset of the literal coders inside the flat probability array.
const NUM_BASE_PROBS: usize = 1984;
/// Probabilities per literal coder.
const LZMA_LIT_SIZE: usize = 0x300;
/// Probability array size in `u16` for the largest allowed `lc + lp` (4).
pub(crate) const NUM_PROBS_MAX: usize = NUM_BASE_PROBS + (LZMA_LIT_SIZE << 4);
/// Initial value of every probability (`kBitModelTotal >> 1`).
pub(crate) const PROB_INIT: u16 = 1024;
/// Zero padding kept after every compressed chunk: the assembly may read up to
/// `LZMA_REQUIRED_INPUT_MAX` (20) bytes past `buf_limit` plus one normalization
/// byte, and reading them is what turns a truncated symbol into an error.
pub(crate) const CHUNK_PAD: usize = 32;
/// Size of the chunk buffer: the largest LZMA2 chunk plus the padding.
const CHUNK_CAP: usize = COMPRESSED_SIZE_MAX as usize + CHUNK_PAD;
/// `remain_len` value meaning "end of payload marker decoded".
pub(crate) const K_MATCH_SPEC_LEN_START: u32 = 274;
/// A range coder whose first 32 bits are `>=` this cannot start with a
/// literal. 7-Zip rejects such a stream up front because the decoder assumes
/// the first symbol of a fresh dictionary is a literal (a rep-match would read
/// before the start of the dictionary), and the assembly relies on it.
pub(crate) const K_BAD_REP_CODE: u32 = 0xBFFF_FC00;

/// Guard bytes after every buffer in debug builds, checked after each call.
#[cfg(debug_assertions)]
const CANARY_LEN: usize = 64;
#[cfg(not(debug_assertions))]
const CANARY_LEN: usize = 0;
const CANARY: u8 = 0xA5;
const CANARY_U16: u16 = 0xA5A5;

#[cfg(target_vendor = "apple")]
macro_rules! sym_prefix {
    () => {
        "_"
    };
}
#[cfg(not(target_vendor = "apple"))]
macro_rules! sym_prefix {
    () => {
        ""
    };
}

/// The exported symbol carries the crate version: two semver-incompatible
/// copies of lzma-rust2 can end up in one binary, and both may enable `asm`.
macro_rules! decode_real_sym {
    () => {
        concat!(
            sym_prefix!(),
            "lzma_rust2_",
            env!("CARGO_PKG_VERSION_MAJOR"),
            "_",
            env!("CARGO_PKG_VERSION_MINOR"),
            "_lzma_dec_decode_real_3"
        )
    };
}

#[cfg(target_vendor = "apple")]
macro_rules! visibility {
    () => {
        concat!(".private_extern ", decode_real_sym!())
    };
}
#[cfg(not(target_vendor = "apple"))]
macro_rules! visibility {
    () => {
        concat!(
            ".hidden ",
            decode_real_sym!(),
            "\n.type ",
            decode_real_sym!(),
            ", %function"
        )
    };
}

global_asm!(
    ".text",
    ".p2align 4,,15",
    concat!(".globl ", decode_real_sym!()),
    visibility!(),
    concat!(decode_real_sym!(), ":"),
    // BTI landing pad; a NOP on cores without branch target identification.
    "hint #34",
    include_str!("../asm/lzma_dec_opt_arm64.s"),
    options(raw),
);

/// Mirror of 7-Zip's `CLzmaProps`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct CLzmaProps {
    pub(crate) lc: u8,
    pub(crate) lp: u8,
    pub(crate) pb: u8,
    pub(crate) _pad: u8,
    /// Dictionary size the distance check uses (`distance < dic_size`).
    pub(crate) dic_size: u32,
}

/// The 96-byte prefix of 7-Zip's `CLzmaDec` that the assembly reads and writes.
///
/// The C struct continues with `numProbs`, `tempBufSize` and `tempBuf`, which
/// only the C wrapper uses; they are not needed here.
#[repr(C)]
#[derive(Debug)]
pub(crate) struct CLzmaDec {
    pub(crate) prop: CLzmaProps,
    pub(crate) probs: *mut u16,
    /// `probs + 1664`; kept for layout fidelity, the arm64 code reads `probs`.
    pub(crate) probs_1664: *mut u16,
    pub(crate) dic: *mut u8,
    pub(crate) dic_buf_size: usize,
    pub(crate) dic_pos: usize,
    pub(crate) buf: *const u8,
    pub(crate) range: u32,
    pub(crate) code: u32,
    pub(crate) processed_pos: u32,
    pub(crate) check_dic_size: u32,
    /// Back-references as "distance + 1".
    pub(crate) reps: [u32; 4],
    pub(crate) state: u32,
    pub(crate) remain_len: u32,
}

// The `.equ offset_*` table in the assembly, verified at compile time.
const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(offset_of!(CLzmaDec, prop) == 0);
    assert!(offset_of!(CLzmaProps, lc) == 0);
    assert!(offset_of!(CLzmaProps, lp) == 1);
    assert!(offset_of!(CLzmaProps, pb) == 2);
    assert!(offset_of!(CLzmaProps, dic_size) == 4);
    assert!(offset_of!(CLzmaDec, probs) == 8);
    assert!(offset_of!(CLzmaDec, probs_1664) == 16);
    assert!(offset_of!(CLzmaDec, dic) == 24);
    assert!(offset_of!(CLzmaDec, dic_buf_size) == 32);
    assert!(offset_of!(CLzmaDec, dic_pos) == 40);
    assert!(offset_of!(CLzmaDec, buf) == 48);
    assert!(offset_of!(CLzmaDec, range) == 56);
    assert!(offset_of!(CLzmaDec, code) == 60);
    assert!(offset_of!(CLzmaDec, processed_pos) == 64);
    assert!(offset_of!(CLzmaDec, check_dic_size) == 68);
    assert!(offset_of!(CLzmaDec, reps) == 72);
    assert!(offset_of!(CLzmaDec, state) == 88);
    assert!(offset_of!(CLzmaDec, remain_len) == 92);
    assert!(size_of::<CLzmaDec>() == 96);
};

/// Calls 7-Zip's `LzmaDec_DecodeReal_3`.
///
/// # Safety
///
/// Every precondition in the module documentation must hold: `p` points to a
/// struct whose `probs`, `dic` and `buf` pointers are valid for the sizes
/// implied by `lc + lp`, `dic_buf_size` and `buf_limit + 20`, and no Rust
/// reference to those buffers is used while the call runs.
#[inline(always)]
pub(crate) unsafe fn decode_real(p: *mut CLzmaDec, limit: usize, buf_limit: *const u8) -> u32 {
    let ret: u64;
    // SAFETY: plain AAPCS64 call. The callee preserves x19..x30 (it saves them
    // on its own frame) and touches only memory reachable from `p`, which the
    // caller guarantees to be valid.
    unsafe {
        asm!(
            concat!("bl ", decode_real_sym!()),
            inout("x0") p as u64 => ret,
            in("x1") limit,
            in("x2") buf_limit,
            clobber_abi("C"),
        );
    }
    ret as u32
}

/// 7-Zip's decoder state plus the buffers it points into.
pub(crate) struct AsmCore {
    dec: CLzmaDec,
    /// Cyclic dictionary window of `dic_buf_size` bytes.
    dic: Vec<u8>,
    /// Flat probability array laid out as 7-Zip's C decoder expects; sized
    /// once for the largest `lc + lp`, like `Lzma2Dec_Allocate` does.
    probs: Vec<u16>,
    /// The current compressed chunk, followed by [`CHUNK_PAD`] zero bytes.
    chunk: Vec<u8>,
    /// Length of the current chunk, range coder init bytes included.
    chunk_len: usize,
    /// Next unread input byte; `RC_INIT_SIZE` right after `load_chunk`.
    in_pos: usize,
    dic_buf_size: usize,
    /// Probabilities, `reps` and `state` must be reinitialized before the next
    /// chunk is decoded (`LzmaDec_InitDicAndState(initState = 1)`).
    need_state_init: bool,
    allocated: bool,
}

// SAFETY: the raw pointers in `dec` are re-derived from the owned buffers
// before every use (`refresh_pointers`), so moving the value between threads
// or sharing it is fine; the assembly keeps no global state.
unsafe impl Send for AsmCore {}
unsafe impl Sync for AsmCore {}

/// Grows `v` to `len` elements of `fill`, followed by the debug canary.
fn allocate<T: Copy>(
    v: &mut Vec<T>,
    len: usize,
    fill: T,
    canary: T,
    msg: &'static str,
) -> crate::Result<()> {
    v.try_reserve_exact(len + CANARY_LEN)
        .map_err(|_| error_out_of_memory(msg))?;
    v.resize(len, fill);
    v.resize(len + CANARY_LEN, canary);
    Ok(())
}

impl AsmCore {
    /// A decoder for a dictionary buffer of `dic_buf_size` bytes (already
    /// rounded like `get_dict_size`), or `None` when the size is outside what
    /// the assembly supports. Buffers are allocated on first use.
    pub(crate) fn new(dic_buf_size: usize) -> Option<Self> {
        if dic_buf_size < DICT_SIZE_MIN as usize || dic_buf_size > u32::MAX as usize {
            return None;
        }
        Some(Self {
            dec: CLzmaDec {
                prop: CLzmaProps {
                    lc: 0,
                    lp: 0,
                    pb: 0,
                    _pad: 0,
                    // The distance check accepts what the portable decoder's
                    // `dist < full` check accepts: the whole buffer.
                    dic_size: dic_buf_size as u32,
                },
                probs: ptr::null_mut(),
                probs_1664: ptr::null_mut(),
                dic: ptr::null_mut(),
                dic_buf_size,
                dic_pos: 0,
                buf: ptr::null(),
                range: 0,
                code: 0,
                processed_pos: 0,
                check_dic_size: 0,
                reps: [1; 4],
                state: 0,
                remain_len: 0,
            },
            dic: Vec::new(),
            probs: Vec::new(),
            chunk: Vec::new(),
            chunk_len: 0,
            in_pos: 0,
            dic_buf_size,
            need_state_init: true,
            allocated: false,
        })
    }

    /// Allocates the dictionary, the probability array and the chunk buffer
    /// on first use, fallibly, so a huge `dict_size` returns an error instead
    /// of aborting the process.
    pub(crate) fn ensure_capacity(&mut self) -> crate::Result<()> {
        if self.allocated {
            return Ok(());
        }
        allocate(
            &mut self.dic,
            self.dic_buf_size,
            0,
            CANARY,
            "dictionary allocation too large",
        )?;
        allocate(
            &mut self.probs,
            NUM_PROBS_MAX,
            PROB_INIT,
            CANARY_U16,
            "probability table allocation failed",
        )?;
        allocate(
            &mut self.chunk,
            CHUNK_CAP,
            0,
            CANARY,
            "chunk buffer allocation failed",
        )?;
        self.allocated = true;
        Ok(())
    }

    /// `LzmaDec_InitDicAndState(initDic = 1)`: the next symbols may not refer
    /// back past this point. Like 7-Zip, neither the window nor `dic_pos` is
    /// touched; the literal context before the first byte reads as 0.
    pub(crate) fn reset_dict(&mut self) {
        self.dec.processed_pos = 0;
        self.dec.check_dic_size = 0;
        self.need_state_init = true;
    }

    /// New `lc` / `lp` / `pb` (control byte `>= 0xC0`); implies a state reset.
    pub(crate) fn set_props(&mut self, lc: u8, lp: u8, pb: u8) -> crate::Result<()> {
        // `lc + lp` bounds the probability array the assembly indexes, so this
        // is checked here again even though the chunk header parser did.
        if lc > 8 || lp > 4 || pb > 4 || lc + lp > 4 {
            return Err(error_invalid_input("corrupted input data (LZMA2:4)"));
        }
        self.dec.prop.lc = lc;
        self.dec.prop.lp = lp;
        self.dec.prop.pb = pb;
        self.need_state_init = true;
        Ok(())
    }

    /// Control byte `0xA0..=0xBF`: probabilities, `reps` and `state` restart.
    pub(crate) fn reset_state(&mut self) {
        self.need_state_init = true;
    }

    /// Reads one compressed chunk of `compressed_size` bytes and initializes
    /// the range coder from its first five bytes.
    pub(crate) fn load_chunk<R: Read>(
        &mut self,
        inner: &mut R,
        compressed_size: usize,
    ) -> crate::Result<()> {
        if compressed_size < RC_INIT_SIZE {
            return Err(error_invalid_input("buffer len must >= 5"));
        }
        if compressed_size > COMPRESSED_SIZE_MAX as usize {
            return Err(error_invalid_input("LZMA2 chunk larger than 64 KiB"));
        }
        self.ensure_capacity()?;
        inner.read_exact(&mut self.chunk[..compressed_size])?;
        self.chunk[compressed_size..compressed_size + CHUNK_PAD].fill(0);
        if self.chunk[0] != 0 {
            return Err(error_invalid_input("first byte is 0"));
        }
        let code = u32::from_be_bytes([self.chunk[1], self.chunk[2], self.chunk[3], self.chunk[4]]);
        if self.dec.check_dic_size == 0 && self.dec.processed_pos == 0 && code >= K_BAD_REP_CODE {
            return Err(error_invalid_data("corrupted input data (LZMA2 range coder)"));
        }
        if self.need_state_init {
            let lc_lp = usize::from(self.dec.prop.lc + self.dec.prop.lp);
            let num_probs = NUM_BASE_PROBS + (LZMA_LIT_SIZE << lc_lp);
            self.probs[..num_probs].fill(PROB_INIT);
            self.dec.reps = [1; 4];
            self.dec.state = 0;
            self.need_state_init = false;
        }
        self.dec.code = code;
        self.dec.range = 0xFFFF_FFFF;
        self.dec.remain_len = 0;
        self.chunk_len = compressed_size;
        self.in_pos = RC_INIT_SIZE;
        Ok(())
    }

    fn refresh_pointers(&mut self) {
        self.dec.probs = self.probs.as_mut_ptr();
        self.dec.probs_1664 = self.probs.as_mut_ptr().wrapping_add(1664);
        self.dec.dic = self.dic.as_mut_ptr();
        self.dec.dic_buf_size = self.dic_buf_size;
        self.dec.buf = self.chunk.as_ptr().wrapping_add(self.in_pos);
    }

    /// Debug-build guard bytes after every buffer are still in place (the
    /// guards are empty in release builds, so this is trivially true there).
    fn canaries_intact(&self) -> bool {
        self.dic[self.dic_buf_size..].iter().all(|&b| b == CANARY)
            && self.probs[NUM_PROBS_MAX..].iter().all(|&p| p == CANARY_U16)
            && self.chunk[CHUNK_CAP..].iter().all(|&b| b == CANARY)
    }

    /// Decodes up to `out.len()` bytes of the current chunk into `out`.
    ///
    /// Fewer bytes come back when the dictionary window ends before `out`
    /// does; the next call continues at the start of the window.
    pub(crate) fn decode(&mut self, out: &mut [u8]) -> crate::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.dec.dic_pos == self.dic_buf_size {
            self.dec.dic_pos = 0;
        }
        let start = self.dec.dic_pos;
        let limit = start + out.len().min(self.dic_buf_size - start);

        loop {
            self.write_rem(limit)?;
            if self.dec.dic_pos >= limit {
                break;
            }

            // `LzmaDec_DecodeReal2`: until the dictionary has been filled once,
            // stop at the point where it is, so `check_dic_size` can be set.
            let mut lim = limit;
            if self.dec.check_dic_size == 0 {
                let rem = (self.dec.prop.dic_size - self.dec.processed_pos) as usize;
                if lim - self.dec.dic_pos > rem {
                    lim = self.dec.dic_pos + rem;
                }
            }

            self.refresh_pointers();
            let base = self.chunk.as_ptr();
            // SAFETY: `refresh_pointers` just derived every pointer from the
            // buffers this struct owns; `lim <= dic_buf_size`; `buf` is inside
            // the chunk and `CHUNK_PAD` zero bytes follow `buf_limit`, more
            // than the 20 the routine may read past it; `lc + lp <= 4` sizes
            // the probability array; no reference into the buffers is alive.
            let res = unsafe { decode_real(&mut self.dec, lim, base.wrapping_add(self.chunk_len)) };
            debug_assert!(self.canaries_intact(), "assembly wrote past a buffer");
            self.in_pos = (self.dec.buf as usize).wrapping_sub(base as usize);
            if res != 0
                || self.in_pos > self.chunk_len
                || self.dec.dic_pos > lim
                || self.dec.remain_len >= K_MATCH_SPEC_LEN_START
            {
                return Err(error_invalid_data("corrupted input data (LZMA2 chunk)"));
            }
            if self.dec.check_dic_size == 0 && self.dec.processed_pos >= self.dec.prop.dic_size {
                self.dec.check_dic_size = self.dec.prop.dic_size;
            }
        }

        let n = self.dec.dic_pos - start;
        out[..n].copy_from_slice(&self.dic[start..start + n]);
        Ok(n)
    }

    /// `LzmaDec_WriteRem`: continues a match the output limit interrupted.
    fn write_rem(&mut self, limit: usize) -> crate::Result<()> {
        let mut len = self.dec.remain_len as usize;
        if len == 0 {
            return Ok(());
        }
        let rem = limit - self.dec.dic_pos;
        if rem < len {
            len = rem;
            if len == 0 {
                return Ok(());
            }
        }
        let dic_buf_size = self.dic_buf_size;
        let rep0 = self.dec.reps[0] as usize;
        if rep0 == 0 || rep0 > dic_buf_size {
            return Err(error_invalid_data("corrupted input data (LZMA2 match)"));
        }
        if self.dec.check_dic_size == 0
            && (self.dec.prop.dic_size - self.dec.processed_pos) as usize <= len
        {
            self.dec.check_dic_size = self.dec.prop.dic_size;
        }
        self.dec.processed_pos = self.dec.processed_pos.wrapping_add(len as u32);
        self.dec.remain_len -= len as u32;

        let mut dic_pos = self.dec.dic_pos;
        let dic = &mut self.dic[..dic_buf_size];
        for _ in 0..len {
            let src = if dic_pos < rep0 {
                dic_pos + dic_buf_size - rep0
            } else {
                dic_pos - rep0
            };
            dic[dic_pos] = dic[src];
            dic_pos += 1;
        }
        self.dec.dic_pos = dic_pos;
        Ok(())
    }

    /// `LzmaDec_UpdateWithUncompressed`: stores up to `out.len()` bytes of an
    /// uncompressed chunk in the window and hands them out.
    pub(crate) fn copy_uncompressed<R: Read>(
        &mut self,
        inner: &mut R,
        out: &mut [u8],
    ) -> crate::Result<usize> {
        self.ensure_capacity()?;
        if self.dec.dic_pos == self.dic_buf_size {
            self.dec.dic_pos = 0;
        }
        let pos = self.dec.dic_pos;
        let n = out.len().min(self.dic_buf_size - pos);
        if n == 0 {
            return Ok(0);
        }
        inner.read_exact(&mut self.dic[pos..pos + n])?;
        out[..n].copy_from_slice(&self.dic[pos..pos + n]);
        if self.dec.check_dic_size == 0
            && (self.dec.prop.dic_size - self.dec.processed_pos) as usize <= n
        {
            self.dec.check_dic_size = self.dec.prop.dic_size;
        }
        self.dec.processed_pos = self.dec.processed_pos.wrapping_add(n as u32);
        self.dec.dic_pos = pos + n;
        Ok(n)
    }

    /// True once the chunk's compressed bytes are all consumed, the range
    /// coder ended on zero and no match is pending: 7-Zip's
    /// `LZMA_STATUS_MAYBE_FINISHED_WITHOUT_MARK` with `packSize == 0`.
    pub(crate) fn chunk_finished(&self) -> bool {
        self.dec.remain_len == 0 && self.dec.code == 0 && self.in_pos == self.chunk_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodes one whole LZMA2 chunk with a hand-filled `CLzmaDec`: proves the
    /// assembler accepted the file, the ABI and the struct layout line up, and
    /// the decoded bytes are right.
    #[cfg(feature = "encoder")]
    #[test]
    fn decodes_a_whole_chunk_through_the_asm() {
        use std::io::Write;

        use crate::{Lzma2Options, Lzma2Writer};

        let text: Vec<u8> = (0..20_000usize)
            .map(|i| b"the quick brown fox jumps over the lazy dog 0123456789 "[i % 55])
            .collect();
        let mut compressed = Vec::new();
        {
            let mut writer = Lzma2Writer::new(&mut compressed, Lzma2Options::with_preset(6));
            writer.write_all(&text).unwrap();
            writer.finish().unwrap();
        }

        // First chunk: control >= 0xE0 (dict + state + props reset).
        let control = compressed[0];
        assert!(control >= 0xE0, "unexpected first control byte {control:#x}");
        let unpacked = (((control & 0x1F) as usize) << 16)
            + u16::from_be_bytes([compressed[1], compressed[2]]) as usize
            + 1;
        let packed = u16::from_be_bytes([compressed[3], compressed[4]]) as usize + 1;
        let props = compressed[5];
        let pb = props / 45;
        let lp = (props % 45) / 9;
        let lc = props % 9;
        assert_eq!(unpacked, text.len(), "the text fits one chunk");
        let payload = &compressed[6..6 + packed];
        assert_eq!(payload[0], 0, "range coder init byte");

        let dic_buf_size = 1usize << 20;
        let mut dic = vec![0u8; dic_buf_size];
        let mut probs = vec![PROB_INIT; NUM_PROBS_MAX];
        let mut chunk = payload.to_vec();
        chunk.resize(packed + CHUNK_PAD, 0);
        let code = u32::from_be_bytes([chunk[1], chunk[2], chunk[3], chunk[4]]);
        assert!(code < K_BAD_REP_CODE);

        let mut dec = CLzmaDec {
            prop: CLzmaProps {
                lc,
                lp,
                pb,
                _pad: 0,
                dic_size: dic_buf_size as u32,
            },
            probs: probs.as_mut_ptr(),
            probs_1664: probs.as_mut_ptr().wrapping_add(1664),
            dic: dic.as_mut_ptr(),
            dic_buf_size,
            dic_pos: 0,
            buf: chunk.as_ptr().wrapping_add(5),
            range: 0xFFFF_FFFF,
            code,
            processed_pos: 0,
            check_dic_size: 0,
            reps: [1; 4],
            state: 0,
            remain_len: 0,
        };
        let data_end = chunk.as_ptr().wrapping_add(packed);

        let mut calls = 0;
        while dec.dic_pos < unpacked {
            assert_eq!(dec.remain_len, 0);
            let res = unsafe { decode_real(&mut dec, unpacked, data_end) };
            calls += 1;
            assert_eq!(res, 0, "SZ_ERROR_DATA after {calls} call(s)");
            assert!(dec.buf as usize <= data_end as usize, "read past the chunk");
            assert!(dec.dic_pos <= unpacked);
            assert!(calls < 10_000, "no progress");
        }

        assert_eq!(dec.dic_pos, unpacked);
        assert_eq!(dec.buf, data_end, "all compressed bytes consumed");
        assert_eq!(dec.code, 0, "range coder finished");
        assert_eq!(dec.remain_len, 0);
        assert!(&dic[..unpacked] == &text[..], "decoded bytes differ");
    }

    #[test]
    fn load_chunk_validates_the_range_coder_init() {
        let mut core = AsmCore::new(4096).unwrap();
        core.set_props(3, 0, 2).unwrap();
        assert!(
            core.load_chunk(&mut &[0u8, 1, 2, 3][..], 4).is_err(),
            "shorter than the five init bytes"
        );
        assert!(
            core.load_chunk(&mut &[1u8, 0, 0, 0, 0][..], 5).is_err(),
            "first byte must be zero"
        );
        assert!(
            core.load_chunk(&mut &[0u8, 0xC0, 0, 0, 0][..], 5).is_err(),
            "kBadRepCode on a fresh dictionary"
        );
        core.load_chunk(&mut &[0u8, 0xBF, 0xFF, 0xFB, 0xFF][..], 5)
            .unwrap();
        assert_eq!(core.dec.code, 0xBFFF_FBFF);
        assert_eq!(core.dec.range, 0xFFFF_FFFF);
        assert_eq!(core.in_pos, RC_INIT_SIZE);
        assert_eq!(core.chunk_len, 5);
        assert!(!core.need_state_init);
        assert_eq!(core.dec.reps, [1; 4]);
        assert_eq!(core.dec.state, 0);
        assert_eq!(&core.chunk[5..5 + CHUNK_PAD], &[0u8; CHUNK_PAD]);
        assert!(core.probs[..NUM_BASE_PROBS + (LZMA_LIT_SIZE << 3)]
            .iter()
            .all(|&p| p == PROB_INIT));
    }

    #[test]
    fn set_props_rejects_out_of_range_values() {
        let mut core = AsmCore::new(4096).unwrap();
        assert!(core.set_props(4, 1, 4).is_err(), "lc + lp > 4");
        assert!(core.set_props(9, 0, 0).is_err());
        assert!(core.set_props(0, 0, 5).is_err());
        assert!(core.set_props(4, 0, 4).is_ok());
        assert!(AsmCore::new(4095).is_none(), "below LZMA_DIC_MIN");
    }

    #[test]
    fn write_rem_copies_across_the_ring_end() {
        let n = 4096usize;
        let mut core = AsmCore::new(n).unwrap();
        core.ensure_capacity().unwrap();
        let original: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        core.dic[..n].copy_from_slice(&original);

        // A full dictionary with a 12-byte match at distance 10 pending, six
        // of its bytes before the end of the ring.
        core.dec.check_dic_size = n as u32;
        core.dec.processed_pos = n as u32;
        core.dec.dic_pos = n - 6;
        core.dec.reps[0] = 10;
        core.dec.remain_len = 12;

        core.write_rem(n).unwrap();
        assert_eq!(core.dec.dic_pos, n);
        assert_eq!(core.dec.remain_len, 6);
        core.dec.dic_pos = 0; // what `decode` does at the ring end
        core.write_rem(n).unwrap();
        assert_eq!(core.dec.dic_pos, 6);
        assert_eq!(core.dec.remain_len, 0);
        assert_eq!(core.dec.processed_pos, n as u32 + 12);

        let mut expected = original;
        let mut pos = n - 6;
        for _ in 0..12 {
            expected[pos] = expected[(pos + n - 10) % n];
            pos = (pos + 1) % n;
        }
        assert!(core.dic[..n] == expected[..]);

        // A bogus distance is an error, not an out-of-bounds access.
        core.dec.reps[0] = n as u32 + 1;
        core.dec.remain_len = 1;
        assert!(core.write_rem(n).is_err());
    }

    #[test]
    fn copy_uncompressed_marks_the_dictionary_full() {
        let n = 4096usize;
        let mut core = AsmCore::new(n).unwrap();
        let data: Vec<u8> = (0..n).map(|i| (i * 7 % 256) as u8).collect();
        let mut src = &data[..];
        let mut out = vec![0u8; n];

        assert_eq!(
            core.copy_uncompressed(&mut src, &mut out[..3000]).unwrap(),
            3000
        );
        assert_eq!(core.dec.check_dic_size, 0);
        assert_eq!(core.dec.processed_pos, 3000);
        assert_eq!(
            core.copy_uncompressed(&mut src, &mut out[3000..]).unwrap(),
            1096
        );
        assert_eq!(core.dec.check_dic_size, n as u32);
        assert_eq!(core.dec.processed_pos, n as u32);
        assert_eq!(core.dec.dic_pos, n);
        assert!(out == data);

        // The ring wraps on the next call.
        let more = [0xEEu8; 8];
        let mut src2 = &more[..];
        assert_eq!(core.copy_uncompressed(&mut src2, &mut out[..8]).unwrap(), 8);
        assert_eq!(core.dec.dic_pos, 8);
        assert_eq!(&core.dic[..8], &more[..]);
        assert!(core.canaries_intact());
    }
}
