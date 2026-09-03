# LZMA / LZMA2 / LZIP / XZ in native Rust

[![Crate](https://img.shields.io/crates/v/lzma-rust2.svg)](https://crates.io/crates/lzma-rust2)
[![Documentation](https://docs.rs/lzma-rust2/badge.svg)](https://docs.rs/lzma-rust2)

LZMA / LZMA2 / LZIP / XZ compression ported from [tukaani xz for java](https://tukaani.org/xz/java.html).

This is a fork of the original, unmaintained lzma-rust crate to continue the development and maintenance.

## Safety

Only the `optimization` feature uses unsafe Rust features to implement optimizations, that are
not possible in safe Rust. Those optimizations are properly guarded and are of course sound.
This includes creation of aligned memory, handwritten assembly code for hot functions and some
pointer logic. Those optimization are well localized and generally consider safe to use, even
with untrusted input.

Deactivating the `optimization` feature will result in 100% standard Rust code.

The opt-in `asm` feature (implies `optimization`) additionally embeds 7-Zip's hand-written assembly LZMA decoder
(public domain, vendored under `src/asm/`; the arm64 source as is, the x86-64 MASM source translated to GNU syntax by
a script) and uses it for LZMA2 streams on 64-bit arm64 and x86-64 targets with Mach-O or ELF object files (Apple,
Linux, Android). The Rust wrapper keeps the decoder inside the buffers it owns: the input chunk is padded, the output
limit is clamped like 7-Zip's own C wrapper does, and every call is checked afterwards. On other targets (Windows
included) the feature does nothing; `LZMA2_ASM_DECODER` tells whether it is active. Differential tests
(`tests/lzma2_asm_diff.rs`) and a fuzz target (`lzma2_diff`) compare it against the portable decoder.

## Performance

The following part is strictly about single threaded performance. It supports multithreaded encoding and decoding
through the `Lzma2WriterMt` and `Lzma2ReaderMt` structs though.

When compared against the `liblzma` crate, which uses the C library of the same name, this crate has improved decoding
speed.

![Decompression Speed LZMA2](./assets/decompression_lzma2.svg)
![Decompression Speed LZMA](./assets/decompression_lzma.svg)

Encoding is also well optimized and is surpassing `liblzma` for level 0 to 3 and matches it for level 4 to 9.

![Compression Speed LZMA2](./assets/compression_lzma2.svg)
![Compression Speed LZMA](./assets/compression_lzma.svg)

Data was assembled using lzma-rust2 v0.4.0 and liblzma v0.4.2.

With the `asm` feature, single-threaded LZMA2 decoding matches 7-Zip itself. On arm64, decoding a 128 MiB LZMA2 stream
(8 independent units, text) on an Apple M4 Pro takes 0.59 s with the assembly decoder, 0.60 s with `7zz t -mmt1` and
0.88 s with the portable decoder (`Lzma2ReaderMt` with 14 threads: 0.09 s, the same as `7zz t`). On x86-64 the
portable decoder is already fast, so the gain is smaller and depends on the data: on an Intel i7-14700KF the crate's
test files (51 MiB of text and executables, ratio 3.7:1) decode at 110 MiB/s with the assembly decoder and 89 MiB/s
with the portable one (1.23x; 1.10x to 1.25x per file), while very repetitive input that decodes to long matches is
slightly slower (40:1 cycled text: 0.89x, the portable decoder copies long matches faster). The LZMA2 decoding itself
takes the same time as `7zz t -mmt1` on the same `.xz` files (0.46 s for the test files); with a CRC64 check the
`XzReader` run is 0.55 s, the difference being this crate's slower CRC64.

## no_std Support

This crate supports `no_std` environments by disabling the default `std` feature.

When used in `no_std` mode, the crate provides custom `Read`, `Write`, and `Error` types
(defined in `no_std.rs`) that are compatible with `no_std` environments. These types offer
similar functionality to their `std::io` counterparts but are implemented using only `core`
and `alloc`.

The custom types include:

- `Error`: A custom error enum with variants for different error conditions.
- `Read`: A trait similar to `std::io::Read`.
- `Write`: A trait similar to `std::io::Write`.

Default implementations for `&[u8]` (Read) and `&mut [u8]` (Write) are provided.

Note that multithreaded features are not available in `no_std` mode as they require
standard library threading primitives.

## License

Licensed under the [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0).
