# 7-Zip assembly LZMA decoder

`lzma_dec_opt_arm64.s` is a preprocessed copy of 7-Zip's hand-written arm64
LZMA decoder, `LzmaDec_DecodeReal_3()`. It is compiled into the crate with
`core::arch::global_asm!` when the `asm` feature is enabled on a supported
arm64 target and drives LZMA2 decoding in `Lzma2Reader` (and therefore
`Lzma2ReaderMt` and the LZMA2 filter of `XzReader`). See `src/lzma_dec_asm.rs`
for the calling contract and the Rust wrapper.

## Provenance and licence

- `upstream/LzmaDecOpt.S` and `upstream/7zAsm.S` are verbatim copies from the
  7-Zip source tree (`Asm/arm64/`), 7-Zip 26.02. `LzmaDecOpt.S` is dated
  2021-04-25 and has not changed since.
- Author: Igor Pavlov. Both files are in the **public domain**, as stated in
  their headers and for the whole LZMA SDK.
- The generated file records the SHA-256 (first 16 hex digits) of both inputs
  in its header, so `git diff` after a regeneration shows exactly what changed.

## Regeneration

```sh
scripts/gen-lzma-dec-asm.sh
```

Needs `clang` (any recent Apple or LLVM clang; `CLANG=` overrides the binary).
The script is deterministic and ends with a self-check that assembles the
output for `arm64-apple-macos11` (with `.subsections_via_symbols`, as rustc
emits it) and `aarch64-unknown-linux-gnu`.

To update to a newer 7-Zip: copy the two files over `upstream/`, diff them
against the previous copies, run the script, and run the differential tests
(`cargo test --release --features asm --test lzma2_asm_diff`).

## Transformations applied by the script

1. Lines starting with `# ` (upstream's shell-style comments) become `// `, so
   the C preprocessor neither rejects them nor substitutes register aliases
   inside them.
2. `clang -E -P -x assembler-with-cpp` resolves `#include "7zAsm.S"`, the
   register `#define`s and the `#ifdef` configuration. The configuration is the
   upstream default: 16-bit probabilities (`_LZMA_PROB32` off) and
   `LZMA_USE_4BYTES_FILL` on.
3. `#pragma`, the `.text` / `.align` / `.p2align` header, `.globl` and the two
   entry labels (`LzmaDec_DecodeReal_3` / `_LzmaDec_DecodeReal_3`) are dropped.
   `src/lzma_dec_asm.rs` emits the section header and an entry label whose name
   carries the crate version (`lzma_rust2_<major>_<minor>_lzma_dec_decode_real_3`,
   with the Mach-O underscore prefix where needed), so two lzma-rust2 versions
   linked into one binary cannot clash.
4. Every internal label and every `.equ` constant is renamed to an
   assembler-local `L*` name. rustc emits `.subsections_via_symbols` for Mach-O,
   and in that mode the assembler rejects a conditional branch to a non-local
   label; on ELF the rename merely keeps those names out of the symbol table.

Nothing else is edited: the instruction stream is the upstream one.
