#!/usr/bin/env bash
# Regenerate src/asm/lzma_dec_opt_arm64.s from the vendored 7-Zip sources in
# src/asm/upstream/ (LzmaDecOpt.S + 7zAsm.S, Igor Pavlov, public domain).
#
# Transformations, in order:
#  1. `# ...` comment lines become `// ...`, so cpp neither trips over them nor
#     substitutes register aliases inside them.
#  2. cpp (`clang -E -P -x assembler-with-cpp`) resolves #include / #define /
#     #ifdef. Configuration is the upstream default: 16-bit probabilities
#     (`_LZMA_PROB32` off), `LZMA_USE_4BYTES_FILL` on.
#  3. Drop `#pragma`, the `.text` / alignment header, `.globl` and the two entry
#     labels: the Rust side (src/lzma_dec_asm.rs) emits those, so the exported
#     symbol can carry the crate version and the right Mach-O / ELF spelling.
#  4. Rename every internal label and `.equ` constant to an assembler-local
#     `L*` name. rustc emits `.subsections_via_symbols` on Mach-O, where a
#     conditional branch to a non-local label is rejected ("conditional branch
#     requires assembler-local label"); on ELF the rename only keeps the
#     symbol table clean.
#  5. Self-check: assemble the result with clang's integrated assembler for
#     Mach-O (with `.subsections_via_symbols`, as rustc will) and for ELF.
#
# Usage: scripts/gen-lzma-dec-asm.sh   (needs clang; CLANG=... to override)
set -euo pipefail
cd "$(dirname "$0")/.."
SRC=src/asm/upstream
OUT=src/asm/lzma_dec_opt_arm64.s
CLANG=${CLANG:-clang}

expanded=$(
    sed -E 's/^([[:space:]]*)#([[:space:]]|$)/\1\/\/\2/' "$SRC/LzmaDecOpt.S" \
        | "$CLANG" -E -P -x assembler-with-cpp -I "$SRC" -
)

labels=$(printf '%s\n' "$expanded" | grep -oE '^[A-Za-z_][A-Za-z0-9_]*:' | tr -d ':' \
    | grep -vE '^_?LzmaDec_DecodeReal_3$' | sort -u)
equs=$(printf '%s\n' "$expanded" | grep -oE '^[[:space:]]*\.equ[[:space:]]+[A-Za-z_][A-Za-z0-9_]*' \
    | awk '{print $2}' | sort -u)
names=$(printf '%s\n' $labels $equs | sort -u | paste -sd'|' -)

{
    printf '// GENERATED FILE - do not edit. Produced by scripts/gen-lzma-dec-asm.sh from\n'
    printf '// src/asm/upstream/LzmaDecOpt.S + 7zAsm.S (7-Zip, Igor Pavlov, public domain).\n'
    printf '// The entry label, .globl and section header are emitted by src/lzma_dec_asm.rs.\n'
    printf '// inputs:'
    shasum -a 256 "$SRC/LzmaDecOpt.S" "$SRC/7zAsm.S" | awk '{printf " %s=%s", $2, substr($1,1,16)}'
    printf '\n'
    printf '%s\n' "$expanded" \
        | sed -E \
            -e '/^[[:space:]]*#pragma/d' \
            -e '/^[[:space:]]*\.text[[:space:]]*$/d' \
            -e '/^[[:space:]]*\.align[[:space:]]+2[[:space:]]*$/d' \
            -e '/^[[:space:]]*\.p2align 4,,15[[:space:]]*$/d' \
            -e '/^[[:space:]]*\.globa?l[[:space:]]/d' \
            -e '/^_?LzmaDec_DecodeReal_3:[[:space:]]*$/d' \
        | perl -pe "s/\\b($names)\\b/L\$1/g"
} > "$OUT"

# Self-check with the integrated assembler, as rustc will see the file.
hdr=$'.text\n.p2align 4,,15\n.globl _selfcheck\n_selfcheck:\nhint #34'
{ printf '%s\n' "$hdr"; cat "$OUT"; printf '.subsections_via_symbols\n'; } \
    | "$CLANG" -c -integrated-as -x assembler -target arm64-apple-macos11 -o /dev/null -
{ printf '%s\n' "${hdr//_selfcheck/selfcheck}"; cat "$OUT"; } \
    | "$CLANG" -c -integrated-as -x assembler -target aarch64-unknown-linux-gnu -o /dev/null -

echo "ok: $OUT ($(wc -l < "$OUT" | tr -d ' ') lines, $(printf '%s\n' "$names" | tr '|' '\n' | wc -l | tr -d ' ') names made local)"
