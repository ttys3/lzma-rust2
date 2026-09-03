//! 7-Zip's hand-written LZMA decoder, used for LZMA2 behind the `asm` feature.
//!
//! On a supported target (`asm` feature, little-endian 64-bit arm64 or x86-64,
//! Mach-O or ELF) [`AsmCore`] wraps the assembly routine
//! `LzmaDec_DecodeReal_3()` from 7-Zip (`src/asm/`, public domain) together
//! with the buffers it points into, and `Lzma2Reader` decodes every LZMA2
//! chunk through it. Everywhere else [`AsmCore`] is an uninhabited placeholder
//! whose constructor returns `None`, so the reader falls back to the portable
//! decoder without any `cfg` noise at its call sites.
//!
//! `common` holds the state and the driver, which are the same for both
//! architectures; `arm64` / `x86_64` embed the assembly and provide the call.

cfg_lzma_asm! {
    if {
        // `sym` renders this private Rust item's fully mangled name, including
        // rustc's crate disambiguator and the platform's symbol prefix. The
        // assembly entry derives its name from it, so every Cargo crate instance
        // gets a distinct external symbol even when name and version are equal.
        fn decode_real_symbol_prefix() {}

        /// The exported symbol carries both the full package version and the
        /// compiler-generated crate disambiguator.
        macro_rules! decode_real_sym {
            () => {
                concat!(
                    "{decode_real_symbol_prefix}_v",
                    env!("CARGO_PKG_VERSION_MAJOR"),
                    "_",
                    env!("CARGO_PKG_VERSION_MINOR"),
                    "_",
                    env!("CARGO_PKG_VERSION_PATCH"),
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

        #[cfg(target_arch = "aarch64")]
        mod arm64;
        #[cfg(target_arch = "aarch64")]
        use arm64 as arch;
        #[cfg(target_arch = "x86_64")]
        mod x86_64;
        #[cfg(target_arch = "x86_64")]
        use x86_64 as arch;

        mod common;
        pub(crate) use common::AsmCore;
    } else {
        mod stub;
        pub(crate) use stub::AsmCore;
    }
}
