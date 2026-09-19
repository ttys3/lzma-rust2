# Why the two architectures take different routes

`README.md` describes *what* each generation script does. This file records
*why* there are two different scripts at all, because the asymmetry looks
arbitrary until you look at what 7-Zip ships.

## Summary

The arm64 routine is embedded essentially as it is, while the x86-64 routine
is translated by `scripts/masm2gas.py` first. That is not a preference. 7-Zip
writes its arm64 decoder in GNU as syntax and its x86-64 decoder in MASM
syntax, and the assembler behind `core::arch::global_asm!` reads only the
former.

## Upstream ships two different assembly dialects

| | arm64 | x86-64 |
| --- | --- | --- |
| Upstream path | `Asm/arm64/LzmaDecOpt.S` | `Asm/x86/LzmaDecOpt.asm` |
| Syntax | GNU as, with C preprocessor | MASM, Intel syntax |
| Comment marker | `//` and `/* */` | `;` |
| Conditionals | `#if` / `#ifdef` (cpp) | `ifdef` / `if` (MASM) |
| Assembled upstream by | a C compiler driver | ml64, uasm or jwasm |
| Vendored `LzmaDecOpt` date | 2021-04-25 | 2024-06-18 |

The arm64 source is written for a C toolchain down to the compiler version
check in its header:

```
$ head -4 upstream/7zAsm.S
// 7zAsm.S -- ASM macros for arm64
// : Igor Pavlov : Public domain

#if defined(__clang_major__) && __clang_major__ >= 20
```

## What `global_asm!` accepts

`core::arch::global_asm!` hands its contents to LLVM's integrated assembler,
which reads GNU as syntax. It does not implement MASM. The difference is not
subtle, and both halves are reproducible with the clang that the generation
scripts already require:

```
$ clang -c -x assembler-with-cpp -I upstream upstream/LzmaDecOpt.S -o /dev/null
$ echo $?
0

$ clang -c -x assembler upstream/LzmaDecOpt.asm -o /dev/null 2>&1 | head -3
upstream/LzmaDecOpt.asm:1:25: error: unexpected token in argument list
; LzmaDecOpt.asm -- ASM version of LzmaDec_DecodeReal_3() function
                        ^
```

The arm64 source assembles cleanly. The x86-64 source fails on its very first
line, before reaching a single instruction: `;` opens a comment in MASM but
separates statements in GNU as, so the assembler reads the file header as
code and trips over the parentheses. Several hundred errors follow. No flag
changes this; the dialect is not implemented.

(Both commands assemble for the host. Run the first one on arm64, or add
`--target=aarch64-unknown-linux-gnu` elsewhere. The error count for the
second depends on the host target, so it is not quoted here: on x86-64 the
instructions are at least recognised once parsing resumes, on arm64 they are
foreign as well.)

## The resulting pipelines

```
arm64                                x86-64
-----                                ------
upstream/LzmaDecOpt.S                upstream/LzmaDecOpt.asm
upstream/7zAsm.S                     upstream/7zAsm.asm
        |                                    |
        | clang -E -P                        | masm2gas.py -D x64 -D ABI_LINUX
        |   -x assembler-with-cpp            |
        |                                    |   translate MASM to GNU as
        |   expand #include "7zAsm.S",       |   Intel syntax, and in the same
        |   #define, resolve #ifdef          |   pass: inline MACRO bodies,
        |                                    |   resolve equ and struct, expand
        |                                    |   7zAsm.asm, evaluate MASM
        |                                    |   ifdef / if
        |                                    |
        +----------------+-------------------+
                         |
                         | rename internal labels and constants to L*
                         |   (Mach-O .subsections_via_symbols rejects a
                         |    conditional branch to a non-local label)
                         |
                         | drop .globl and the upstream entry label
                         |   (src/lzma_dec_asm/{arm64,x86_64}.rs emit a
                         |    crate-disambiguated, versioned one instead)
                         |
                         | self-check: clang -c -integrated-as, once for
                         |   Mach-O and once for ELF
                         v
              lzma_dec_opt_{arm64,x86_64}.s   (committed)
```

Note that the expansion stage is not shared, and cannot be. The arm64 source
is configured with C preprocessor directives, so a C preprocessor resolves
them. The x86-64 source is configured with MASM's own `ifdef` / `if` and
`MACRO`, which `cpp` cannot evaluate, so `masm2gas.py` does that work itself
while translating. Only the symbol renaming and the self-check are common to
both paths.

## Why not assemble the MASM source with uasm or jwasm instead

Upstream does not document a position on this, so what follows is the
trade-off as it stands, not a reconstruction of anyone's reasoning.

Keeping `LzmaDecOpt.asm` in MASM and calling uasm or jwasm from a build
script would avoid writing a translator, at these costs:

- `cargo build` would stop being self-contained. Every downstream build, CI
  job and vendored offline build would need an assembler that is not part of
  the Rust toolchain and that most distributions do not install by default.
- The failure mode moves from this repository to each user's machine, and
  arrives as a build error in a dependency rather than as a diff here.

Translating offline instead moves the work to a step that runs when someone
updates the vendored 7-Zip sources. The output is committed, so it is
reviewable in `git diff`, and CI checks it is reproducible:

```yaml
- name: Check the generated assembly is up to date
  if: matrix.os == 'macos-latest' || matrix.os == 'ubuntu-latest'
  run: scripts/gen-lzma-dec-asm.sh && scripts/gen-lzma-dec-asm-x86_64.sh && git diff --exit-code src/asm
```

Each generated file also records the SHA-256 prefix of its inputs in its
header, so a regeneration that picks up changed upstream sources is visible
as a header change rather than only as a wall of instruction diffs.

## What holds on both paths

Neither script rewrites the algorithm. They resolve what the upstream build
system would have resolved (includes, macros, configuration) and rename
symbols so the result can live inside a Rust crate. The instruction stream is
the upstream one; only its spelling changes.

Both paths are covered by the same differential tests against the portable
decoder, `tests/lzma2_asm_diff.rs`, and by the `lzma2_diff` fuzz target. The
x86-64 translation carries more risk than the arm64 copy precisely because it
is a translation, which is the reason to run those tests on both
architectures before trusting a regeneration.
