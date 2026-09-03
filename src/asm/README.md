# 7-Zip assembly LZMA decoder

`lzma_dec_opt_arm64.s` and `lzma_dec_opt_x86_64.s` are copies of 7-Zip's
hand-written LZMA decoder, `LzmaDec_DecodeReal_3()`, for arm64 and x86-64.
They are compiled into the crate with `core::arch::global_asm!` when the `asm`
feature is enabled on a supported target and drive LZMA2 decoding in
`Lzma2Reader` (and therefore `Lzma2ReaderMt` and the LZMA2 filter of
`XzReader`). See `src/lzma_dec_asm/common.rs` for the calling contract and the
Rust wrapper, `src/lzma_dec_asm/arm64.rs` / `x86_64.rs` for the entry points.

## Provenance and licence

- `upstream/LzmaDecOpt.S` and `upstream/7zAsm.S` are verbatim copies from the
  7-Zip source tree (`Asm/arm64/`), 7-Zip 26.02. `LzmaDecOpt.S` is dated
  2021-04-25 and has not changed since.
- `upstream/LzmaDecOpt.asm` and `upstream/7zAsm.asm` are verbatim copies from
  `Asm/x86/` of the same release, in MASM syntax (dated 2024-06-18 and
  2023-12-08).
- Author: Igor Pavlov. All four files are in the **public domain**, as stated
  in their headers and for the whole LZMA SDK.
- Each generated file records the SHA-256 (first 16 hex digits) of its inputs
  in its header, so `git diff` after a regeneration shows exactly what changed.

## Regeneration

```sh
scripts/gen-lzma-dec-asm.sh          # arm64
scripts/gen-lzma-dec-asm-x86_64.sh   # x86-64
```

Both need `clang` (any recent Apple or LLVM clang; `CLANG=` overrides the
binary); the x86-64 script also needs `python3`. The scripts are deterministic
and end with a self-check that assembles the output for Mach-O (with
`.subsections_via_symbols`, as rustc emits it) and for ELF; the x86-64 script
also assembles with GNU as when it is installed.

To update to a newer 7-Zip: copy the files over `upstream/`, diff them against
the previous copies, run both scripts, and run the differential tests on both
architectures (`cargo test --release --features asm --test lzma2_asm_diff`,
plus a soak: `LZMA2_DIFF_SOAK_SECS=3600 cargo test --release --features asm
--test lzma2_asm_diff -- --ignored soak`).

## arm64: transformations applied by the script

1. Lines starting with `# ` (upstream's shell-style comments) become `// `, so
   the C preprocessor neither rejects them nor substitutes register aliases
   inside them.
2. `clang -E -P -x assembler-with-cpp` resolves `#include "7zAsm.S"`, the
   register `#define`s and the `#ifdef` configuration. The configuration is the
   upstream default: 16-bit probabilities (`_LZMA_PROB32` off) and
   `LZMA_USE_4BYTES_FILL` on.
3. `#pragma`, the `.text` / `.align` / `.p2align` header, `.globl` and the two
   entry labels (`LzmaDec_DecodeReal_3` / `_LzmaDec_DecodeReal_3`) are dropped.
   `src/lzma_dec_asm/arm64.rs` emits the section header and an entry label whose
   name carries the crate version
   (`lzma_rust2_<major>_<minor>_lzma_dec_decode_real_3`, with the Mach-O
   underscore prefix where needed), so two lzma-rust2 versions linked into one
   binary cannot clash.
4. Every internal label and every `.equ` constant is renamed to an
   assembler-local `L*` name. rustc emits `.subsections_via_symbols` for Mach-O,
   and in that mode the assembler rejects a conditional branch to a non-local
   label; on ELF the rename merely keeps those names out of the symbol table.

Nothing else is edited: the instruction stream is the upstream one.

## x86-64: translation from MASM

Upstream assembles `LzmaDecOpt.asm` with ml64 / uasm / jwasm. Neither GNU as
nor LLVM's integrated assembler reads MASM, so `scripts/masm2gas.py` translates
the MASM subset those two files use into fully expanded GNU as Intel syntax.
Every construct is translated by rule (the script's docstring is the complete
list), and an identifier the script cannot resolve is an error, not a
pass-through:

- `equ` register aliases (`cod` → `x5` → `ebp`) are resolved to the final
  register. `equ` constants become a `.set` table with `L`-prefixed names and
  stay symbolic at their uses (`LIsMatch * LPMULT`); `SHL` / `SHR` become
  `<<` / `>>`.
- `struct` definitions become `.set` offsets, and `GLOB_2 field` / `LOC field`
  accesses become `size ptr [reg + LCLzmaDec_Asm_field]`, with the operand size
  taken from the field type as MASM does. `CLzmaDec_Asm` is the same 96-byte
  struct the arm64 code uses; the x86-64 code addresses the probability array
  through its `probs_1664` member.
- Macros are expanded inline (parameters replaced token-wise, `@CatStr`
  concatenated). Each top-level macro call is echoed as a `#` comment above its
  expansion, so the output can be read against the upstream source.
- `ifdef` / `if` are evaluated with `x64` and `ABI_LINUX` defined: the
  System V ABI, parameters in rdi / rsi / rdx, rbx / rbp / r12–r15 saved. The
  Win64 branches of the upstream source are not emitted, and the feature is
  compiled out on Windows.
- `@@:` / `@F` / `@B` become GNU numeric local labels `1:` / `1f` / `1b`;
  every other label gets the assembler-local `L` prefix (same reason as on
  arm64).
- `MY_PROC` / `MY_ENDP`, `SEGMENT`, `OPTION`, `PROC` / `ENDP` are dropped.
  `src/lzma_dec_asm/x86_64.rs` emits the section header, a 64-byte aligned
  entry (upstream places the routine in a 64-byte aligned segment) with an
  `endbr64` landing pad, and the versioned symbol; `MY_ENDP` becomes `ret`.
- `align N` → `.p2align log2(N)`; `SHORT` / `near ptr` are dropped (the
  assembler picks the encoding); `123h` → `0x123`; mnemonics and registers are
  lowercased.

The instruction stream is the upstream one; only its spelling changes.
