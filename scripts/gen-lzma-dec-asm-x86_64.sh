#!/usr/bin/env bash
# Regenerate src/asm/lzma_dec_opt_x86_64.s from the vendored 7-Zip sources in
# src/asm/upstream/ (LzmaDecOpt.asm + 7zAsm.asm, Igor Pavlov, public domain).
#
# Upstream is MASM (assembled by ml64 / uasm / jwasm), which neither GNU as nor
# LLVM's integrated assembler reads, so scripts/masm2gas.py translates it by
# rule: macros and register aliases are expanded, `equ` constants and struct
# fields become a `.set` table, every label and constant gets an
# assembler-local `L*` name (rustc emits `.subsections_via_symbols` on Mach-O,
# where a branch to a non-local label is rejected), and the ABI is fixed to
# System V (`x64` + `ABI_LINUX`, i.e. rdi / rsi / rdx parameters and rbx, rbp,
# r12-r15 saved). The entry label, .globl and section header are emitted by
# src/lzma_dec_asm/x86_64.rs, so the exported symbol can carry the crate
# version and the right Mach-O / ELF spelling.
#
# Self-check: assemble the result with clang's integrated assembler for ELF and
# for Mach-O (with `.subsections_via_symbols`, as rustc will), and with GNU as
# when it is installed. GNU as pads `.p2align` with different NOP sequences, so
# the object files are not byte-compared; the instruction stream is the same.
#
# Usage: scripts/gen-lzma-dec-asm-x86_64.sh   (needs python3 and clang;
#        PYTHON=... / CLANG=... to override)
set -euo pipefail
cd "$(dirname "$0")/.."
SRC=src/asm/upstream
OUT=src/asm/lzma_dec_opt_x86_64.s
CLANG=${CLANG:-clang}
PYTHON=${PYTHON:-python3}

{
    printf '# GENERATED FILE - do not edit. Produced by scripts/gen-lzma-dec-asm-x86_64.sh\n'
    printf '# (scripts/masm2gas.py) from src/asm/upstream/LzmaDecOpt.asm + 7zAsm.asm\n'
    printf '# (7-Zip, Igor Pavlov, public domain), translated from MASM to GNU as Intel\n'
    printf '# syntax for the System V AMD64 ABI. The entry label, .globl and section\n'
    printf '# header are emitted by src/lzma_dec_asm/x86_64.rs.\n'
    printf '# inputs:'
    shasum -a 256 "$SRC/LzmaDecOpt.asm" "$SRC/7zAsm.asm" | awk '{printf " %s=%s", $2, substr($1,1,16)}'
    printf '\n\n'
    "$PYTHON" scripts/masm2gas.py -D x64 -D ABI_LINUX "$SRC/LzmaDecOpt.asm"
} > "$OUT"

# Self-check with the integrated assembler, as rustc will see the file.
hdr=$'.intel_syntax noprefix\n.text\n.p2align 6\n.globl _selfcheck\n_selfcheck:\nendbr64'
{ printf '%s\n' "$hdr"; cat "$OUT"; printf '.subsections_via_symbols\n'; } \
    | "$CLANG" -c -integrated-as -x assembler -target x86_64-apple-macos11 -o /dev/null -
{ printf '%s\n' "${hdr//_selfcheck/selfcheck}"; cat "$OUT"; } \
    | "$CLANG" -c -integrated-as -x assembler -target x86_64-unknown-linux-gnu -o /dev/null -
if command -v as >/dev/null && as --version 2>/dev/null | grep -q 'GNU assembler'; then
    { printf '%s\n' "${hdr//_selfcheck/selfcheck}"; cat "$OUT"; } | as --64 -o /dev/null -
    gnu=", GNU as"
fi

echo "ok: $OUT ($(wc -l < "$OUT" | tr -d ' ') lines; assembled for Mach-O, ELF${gnu:-})"
