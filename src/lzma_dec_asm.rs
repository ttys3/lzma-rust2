//! 7-Zip's hand-written arm64 LZMA decoder, used for LZMA2 behind the `asm`
//! feature.
//!
//! On a supported target (`asm` feature, little-endian arm64, Mach-O or ELF)
//! [`AsmCore`] wraps the assembly routine `LzmaDec_DecodeReal_3()` from 7-Zip
//! (`src/asm/`, public domain) together with the buffers it points into, and
//! `Lzma2Reader` decodes every LZMA2 chunk through it. Everywhere else
//! [`AsmCore`] is an uninhabited placeholder whose constructor returns `None`,
//! so the reader falls back to the portable decoder without any `cfg` noise
//! at its call sites.

cfg_lzma_asm! {
    if {
        mod arm64;
        pub(crate) use arm64::AsmCore;
    } else {
        mod stub;
        pub(crate) use stub::AsmCore;
    }
}
