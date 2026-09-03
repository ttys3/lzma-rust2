//! Placeholder for targets without the assembly decoder.
//!
//! `AsmCore` is uninhabited here: `Lzma2Core::Asm` still exists as a variant,
//! but it can never be constructed, so every arm that handles it is dead code
//! the compiler removes.

use crate::Read;

/// Never constructed on this target; see [`AsmCore::new`].
pub(crate) enum AsmCore {}

impl AsmCore {
    /// Always `None`: the assembly decoder is not available on this target.
    pub(crate) fn new(_dic_buf_size: usize) -> Option<Self> {
        None
    }

    pub(crate) fn ensure_capacity(&mut self) -> crate::Result<()> {
        match *self {}
    }

    pub(crate) fn reset_dict(&mut self) {
        match *self {}
    }

    pub(crate) fn set_props(&mut self, _lc: u8, _lp: u8, _pb: u8) -> crate::Result<()> {
        match *self {}
    }

    pub(crate) fn reset_state(&mut self) {
        match *self {}
    }

    pub(crate) fn load_chunk<R: Read>(
        &mut self,
        _inner: &mut R,
        _compressed_size: usize,
    ) -> crate::Result<()> {
        match *self {}
    }

    pub(crate) fn decode(&mut self, _out: &mut [u8]) -> crate::Result<usize> {
        match *self {}
    }

    pub(crate) fn copy_uncompressed<R: Read>(
        &mut self,
        _inner: &mut R,
        _out: &mut [u8],
    ) -> crate::Result<usize> {
        match *self {}
    }

    pub(crate) fn chunk_finished(&self) -> bool {
        match *self {}
    }
}
