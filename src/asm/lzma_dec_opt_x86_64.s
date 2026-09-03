# GENERATED FILE - do not edit. Produced by scripts/gen-lzma-dec-asm-x86_64.sh
# (scripts/masm2gas.py) from src/asm/upstream/LzmaDecOpt.asm + 7zAsm.asm
# (7-Zip, Igor Pavlov, public domain), translated from MASM to GNU as Intel
# syntax for the System V AMD64 ABI. The entry label, .globl and section
# header are emitted by src/lzma_dec_asm/x86_64.rs.
# inputs: src/asm/upstream/LzmaDecOpt.asm=bddfb31a59c49c8f src/asm/upstream/7zAsm.asm=8a06bb3e5d26ed5b

# ---- constants ----
.set LPSHIFT, 1
.set LPMULT, (1 << LPSHIFT)
.set LPMULT_HALF, (1 << (LPSHIFT - 1))
.set LPMULT_2, (1 << (LPSHIFT + 1))
.set LkMatchSpecLen_Error_Data, (1 << 9)
.set LkNumBitModelTotalBits, 11
.set LkBitModelTotal, (1 << LkNumBitModelTotalBits)
.set LkNumMoveBits, 5
.set LkBitModelOffset, ((1 << LkNumMoveBits) - 1)
.set LkTopValue, (1 << 24)
.set LkNumPosBitsMax, 4
.set LkNumPosStatesMax, (1 << LkNumPosBitsMax)
.set LkLenNumLowBits, 3
.set LkLenNumLowSymbols, (1 << LkLenNumLowBits)
.set LkLenNumHighBits, 8
.set LkLenNumHighSymbols, (1 << LkLenNumHighBits)
.set LkNumLenProbs, (2 * LkLenNumLowSymbols * LkNumPosStatesMax + LkLenNumHighSymbols)
.set LLenLow, 0
.set LLenHigh, (LLenLow + 2 * LkLenNumLowSymbols * LkNumPosStatesMax)
.set LkNumStates, 12
.set LkNumStates2, 16
.set LkNumLitStates, 7
.set LkEndPosModelIndex, 14
.set LkNumFullDistances, (1 << (LkEndPosModelIndex >> 1))
.set LkNumPosSlotBits, 6
.set LkNumLenToPosStates, 4
.set LkNumAlignBits, 4
.set LkAlignTableSize, (1 << LkNumAlignBits)
.set LkMatchMinLen, 2
.set LkMatchSpecLenStart, (LkMatchMinLen + LkLenNumLowSymbols * 2 + LkLenNumHighSymbols)
.set LkStartOffset, 1664
.set LSpecPos, (-LkStartOffset)
.set LIsRep0Long, (LSpecPos + LkNumFullDistances)
.set LRepLenCoder, (LIsRep0Long + (LkNumStates2 << LkNumPosBitsMax))
.set LLenCoder, (LRepLenCoder + LkNumLenProbs)
.set LIsMatch, (LLenCoder + LkNumLenProbs)
.set LkAlign, (LIsMatch + (LkNumStates2 << LkNumPosBitsMax))
.set LIsRep, (LkAlign + LkAlignTableSize)
.set LIsRepG0, (LIsRep + LkNumStates)
.set LIsRepG1, (LIsRepG0 + LkNumStates)
.set LIsRepG2, (LIsRepG1 + LkNumStates)
.set LPosSlot, (LIsRepG2 + LkNumStates)
.set LLiteral, (LPosSlot + (LkNumLenToPosStates << LkNumPosSlotBits))

# ---- struct offsets ----
.set LCLzmaDec_Asm_lc, 0
.set LCLzmaDec_Asm_lp, 1
.set LCLzmaDec_Asm_pb, 2
.set LCLzmaDec_Asm_probs_1664, 16
.set LCLzmaDec_Asm_dic_Spec, 24
.set LCLzmaDec_Asm_dicBufSize, 32
.set LCLzmaDec_Asm_dicPos_Spec, 40
.set LCLzmaDec_Asm_buf_Spec, 48
.set LCLzmaDec_Asm_range_Spec, 56
.set LCLzmaDec_Asm_code_Spec, 60
.set LCLzmaDec_Asm_processedPos_Spec, 64
.set LCLzmaDec_Asm_checkDicSize, 68
.set LCLzmaDec_Asm_rep0, 72
.set LCLzmaDec_Asm_rep1, 76
.set LCLzmaDec_Asm_rep2, 80
.set LCLzmaDec_Asm_rep3, 84
.set LCLzmaDec_Asm_state_Spec, 88
.set LCLzmaDec_Asm_remainLen, 92
.set LCLzmaDec_Asm_Loc_OLD_RSP, 0
.set LCLzmaDec_Asm_Loc_lzmaPtr, 8
.set LCLzmaDec_Asm_Loc_dicBufSize, 40
.set LCLzmaDec_Asm_Loc_probs_Spec, 48
.set LCLzmaDec_Asm_Loc_dic_Spec, 56
.set LCLzmaDec_Asm_Loc_limit, 64
.set LCLzmaDec_Asm_Loc_bufLimit, 72
.set LCLzmaDec_Asm_Loc_lc2, 80
.set LCLzmaDec_Asm_Loc_lpMask, 84
.set LCLzmaDec_Asm_Loc_pbMask, 88
.set LCLzmaDec_Asm_Loc_checkDicSize, 92
.set LCLzmaDec_Asm_Loc_remainLen, 100
.set LCLzmaDec_Asm_Loc_dicPos_Spec, 104
.set LCLzmaDec_Asm_Loc_rep0, 112
.set LCLzmaDec_Asm_Loc_rep1, 116
.set LCLzmaDec_Asm_Loc_rep2, 120
.set LCLzmaDec_Asm_Loc_rep3, 124
.set Lsizeof_CLzmaDec_Asm_Loc, 128

# ---- code ----
# LzmaDecOpt.asm -- ASM version of LzmaDec_DecodeReal_3() function
# 2024-06-18: Igor Pavlov : Public domain

# 3 - is the code compatibility version of LzmaDec_DecodeReal_*()
# function for check at link time.
# That code is tightly coupled with LzmaDec_TryDummy()
# and with another functions in LzmaDec.c file.
# CLzmaDec structure, (probs) array layout, input and output of
# LzmaDec_DecodeReal_*() must be equal in both versions (C / ASM).

# 7zAsm.asm -- ASM macros
# 2023-12-08 : Igor Pavlov : Public domain

# UASM can require these changes
# OPTION FRAMEPRESERVEFLAGS:ON
# OPTION PROLOGUE:NONE
# OPTION EPILOGUE:NONE

#  r0_L equ AL
#  r1_L equ CL
#  r2_L equ DL
#  r3_L equ BL

#  r0_H equ AH
#  r1_H equ CH
#  r2_H equ DH
#  r3_H equ BH

# for fastcall and for WIN-x64

# x64

# for LINUX-x64:

# if Z7_LZMA_DEC_OPT_ASM_USE_SEGMENT is     defined, we use additional SEGMENT with 64-byte alignment.
# if Z7_LZMA_DEC_OPT_ASM_USE_SEGMENT is not defined, we use default SEGMENT (where default 16-byte alignment of segment is expected).
# The performance is almost identical in our tests.
# But the performance can depend from position of lzmadec code inside instruction cache
# or micro-op cache line (depending from low address bits in 32-byte/64-byte cache lines).
# And 64-byte alignment provides a more consistent speed regardless
# of the code's position in the executable.
# But also it's possible that code without Z7_LZMA_DEC_OPT_ASM_USE_SEGMENT can be
# slightly faster than 64-bytes aligned code in some cases, if offset of lzmadec
# code in 64-byte block after compilation provides better speed by some reason.
# Note that Z7_LZMA_DEC_OPT_ASM_USE_SEGMENT adds an extra section to the ELF file.
# If you don't want to get that extra section, do not define Z7_LZMA_DEC_OPT_ASM_USE_SEGMENT.

# _LZMA_SIZE_OPT  equ 1

# _LZMA_PROB32 equ 1

#       x0      range
#       x1      pbPos / (prob) TREE
#       x2      probBranch / prm (MATCHED) / pbPos / cnt
#       x3      sym
#====== r4 ===  RSP
#       x5      cod
#       x6      t1 NORM_CALC / probs_state / dist
#       x7      t0 NORM_CALC / prob2 IF_BIT_1
#       x8      state
#       x9      match (MATCHED) / sym2 / dist2 / lpMask_reg
#       x10     kBitModelTotal_reg
#       r11     probs
#       x12     offs (MATCHED) / dic / len_temp
#       x13     processedPos
#       x14     bit (MATCHED) / dicPos
#       r15     buf

# ---------- Branch MACROS ----------

# ---------- CMOV MACROS ----------

# ---------- MATCHED LITERAL ----------

# ---------- REVERSE BITS ----------

# RSP is (16x + 8) bytes aligned in WIN64-x64
# LocalSize equ ((((SIZEOF CLzmaDec_Asm_Loc) + 7) / 16 * 16) + 8)

# MY_ALIGN_64
# MY_PROC LzmaDec_DecodeReal_3, 3: entry label emitted by the includer
# MY_PUSH_PRESERVED_ABI_REGS
        push    rbx
        push    rbp
        push    r12
        push    r13
        push    r14
        push    r15

        lea     rax, [rsp - (Lsizeof_CLzmaDec_Asm_Loc)]
        and     rax, -128
        mov     rbp, rsp
        mov     rsp, rax
        mov     qword ptr [rax + LCLzmaDec_Asm_Loc_OLD_RSP], rbp
        mov     qword ptr [rax + LCLzmaDec_Asm_Loc_lzmaPtr], rdi

        mov     dword ptr [rax + LCLzmaDec_Asm_Loc_remainLen], 0 # remainLen must be ZERO

        mov     qword ptr [rax + LCLzmaDec_Asm_Loc_bufLimit], rdx
        mov     rbx, rdi #  CLzmaDec_Asm_Loc pointer for GLOB_2
        mov     r12, qword ptr [rbx + LCLzmaDec_Asm_dic_Spec]
        add     rsi, r12
        mov     qword ptr [rax + LCLzmaDec_Asm_Loc_limit], rsi

# COPY_VAR(rep0)
        mov     edi, dword ptr [rbx + LCLzmaDec_Asm_rep0]
        mov     dword ptr [rax + LCLzmaDec_Asm_Loc_rep0], edi
# COPY_VAR(rep1)
        mov     edi, dword ptr [rbx + LCLzmaDec_Asm_rep1]
        mov     dword ptr [rax + LCLzmaDec_Asm_Loc_rep1], edi
# COPY_VAR(rep2)
        mov     edi, dword ptr [rbx + LCLzmaDec_Asm_rep2]
        mov     dword ptr [rax + LCLzmaDec_Asm_Loc_rep2], edi
# COPY_VAR(rep3)
        mov     edi, dword ptr [rbx + LCLzmaDec_Asm_rep3]
        mov     dword ptr [rax + LCLzmaDec_Asm_Loc_rep3], edi

        mov     r14, qword ptr [rbx + LCLzmaDec_Asm_dicPos_Spec]
        add     r14, r12
        mov     qword ptr [rax + LCLzmaDec_Asm_Loc_dicPos_Spec], r14
        mov     qword ptr [rax + LCLzmaDec_Asm_Loc_dic_Spec], r12

        mov     cl, byte ptr [rbx + LCLzmaDec_Asm_pb]
        mov     edi, 1
        shl     edi, cl
        dec     edi
        mov     dword ptr [rax + LCLzmaDec_Asm_Loc_pbMask], edi

# unsigned pbMask = ((unsigned)1 << (p->prop.pb)) - 1;
# unsigned lc = p->prop.lc;
# unsigned lpMask = ((unsigned)0x100 << p->prop.lp) - ((unsigned)0x100 >> lc);

        mov     cl, byte ptr [rbx + LCLzmaDec_Asm_lc]
        mov     edx, 0x100
        mov     edi, edx
        shr     edx, cl
# inc     x1
        add     cl, LPSHIFT
        mov     dword ptr [rax + LCLzmaDec_Asm_Loc_lc2], ecx
        mov     cl, byte ptr [rbx + LCLzmaDec_Asm_lp]
        shl     edi, cl
        sub     edi, edx
        mov     dword ptr [rax + LCLzmaDec_Asm_Loc_lpMask], edi
        mov     r9d, edi

# mov     probs, GLOB_2 probs_Spec
# add     probs, kStartOffset SHL PSHIFT
        mov     r11, qword ptr [rbx + LCLzmaDec_Asm_probs_1664]
        mov     qword ptr [rax + LCLzmaDec_Asm_Loc_probs_Spec], r11

        mov     rdi, qword ptr [rbx + LCLzmaDec_Asm_dicBufSize]
        mov     qword ptr [rax + LCLzmaDec_Asm_Loc_dicBufSize], rdi

        mov     ecx, dword ptr [rbx + LCLzmaDec_Asm_checkDicSize]
        mov     dword ptr [rax + LCLzmaDec_Asm_Loc_checkDicSize], ecx

        mov     r13d, dword ptr [rbx + LCLzmaDec_Asm_processedPos_Spec]

        mov     r8d, dword ptr [rbx + LCLzmaDec_Asm_state_Spec]
        shl     r8d, LPSHIFT

        mov     r15,   qword ptr [rbx + LCLzmaDec_Asm_buf_Spec]
        mov     eax, dword ptr [rbx + LCLzmaDec_Asm_range_Spec]
        mov     ebp,   dword ptr [rbx + LCLzmaDec_Asm_code_Spec]
        mov     r10d, LkBitModelTotal
        xor     ebx, ebx

# if (processedPos != 0 || checkDicSize != 0)
        or      ecx, r13d
        jz      1f

        add     rdi, r12
        cmp     r14, r12
        cmovnz  rdi, r14
        movzx   ebx, byte ptr[rdi - 1]

1:
# IsMatchBranch_Pre
        mov     ecx, dword ptr [rsp + LCLzmaDec_Asm_Loc_pbMask]
        and     ecx, r13d
        shl     ecx, (LkLenNumLowBits + 1 + LPSHIFT)
        lea     rsi, [r11 + 1 * r8]
        cmp     r8d, 4 * LPMULT
        jb      Llit_end
        cmp     r8d, LkNumLitStates * LPMULT
        jb      Llit_matched_end
        jmp     Llz_end

# ---------- LITERAL ----------
        .p2align 6
Llit_start:
        xor     r8d, r8d
Llit_start_2:
# LIT_PROBS lpMask_reg
        mov     edi, r13d
        shl     edi, 8
        add     ebx, edi
        and     ebx, r9d
        add     rsi, rcx
        mov     ecx, dword ptr [rsp + LCLzmaDec_Asm_Loc_lc2]
        lea     ebx, dword ptr[rbx + 2 * rbx]
        add     r11, LLiteral * LPMULT
        shl     ebx, cl
        add     r11, rbx
        mov     edi, r10d
        sub     edi, edx
        shr     edi, LkNumMoveBits
        add     edx, edi
        mov     word ptr [0 * 1 + rsi + LIsMatch * LPMULT], dx
        inc     r13d

# BIT_0   x1, x2
        movzx   ecx, word ptr [r11 + 1 * LPMULT]
        movzx   edx, word ptr [r11 + 1 * LPMULT_2]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + 1 * LPMULT_2 + LPMULT]
        cmovae  edx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        cmovb   edi, r10d
        mov     ebx, 2
        sbb     ebx, 0 - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [r11 + 1 * LPMULT], di
# BIT_1   x2, x1
        movzx   ecx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  ecx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di
# BIT_1   x1, x2
        movzx   edx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  edx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di
# BIT_1   x2, x1
        movzx   ecx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  ecx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di
# BIT_1   x1, x2
        movzx   edx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  edx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di
# BIT_1   x2, x1
        movzx   ecx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  ecx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di
# BIT_1   x1, x2
        movzx   edx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  edx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di

# BIT_2   x2, 256 - 1
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 256 - 1
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di

# mov     dic, LOC dic_Spec
        mov     r11, qword ptr [rsp + LCLzmaDec_Asm_Loc_probs_Spec]
# IsMatchBranch_Pre
        mov     ecx, dword ptr [rsp + LCLzmaDec_Asm_Loc_pbMask]
        and     ecx, r13d
        shl     ecx, (LkLenNumLowBits + 1 + LPSHIFT)
        lea     rsi, [r11 + 1 * r8]
        mov     byte ptr[r14], bl
        inc     r14

# CheckLimits
        cmp     r15, qword ptr [rsp + LCLzmaDec_Asm_Loc_bufLimit]
        jae     Lfin_OK
        cmp     r14, qword ptr [rsp + LCLzmaDec_Asm_Loc_limit]
        jae     Lfin_OK
Llit_end:
# IF_BIT_0_NOUP probs_state_R, pbPos_R, IsMatch, lit_start
        movzx   edx, word ptr [rcx * 1 + rsi + LIsMatch * LPMULT]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        cmp     ebp, eax
        jb      Llit_start

# jmp     IsMatch_label

# ---------- MATCHES ----------
# MY_ALIGN_32
LIsMatch_label:
# UPDATE_1 probs_state_R, pbPos_R, IsMatch
        sub     edi, eax
        sub     ebp, eax
        mov     eax, edi
        mov     edi, edx
        shr     edx, LkNumMoveBits
        sub     edi, edx
        mov     word ptr [rcx * 1 + rsi + LIsMatch * LPMULT], di
# IF_BIT_1 probs_state_R, 0, IsRep, IsRep_label
        movzx   edx, word ptr [0 * 1 + rsi + LIsRep * LPMULT]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        cmp     ebp, eax
        jae     LIsRep_label
        mov     edi, r10d
        sub     edi, edx
        shr     edi, LkNumMoveBits
        add     edx, edi
        mov     word ptr [0 * 1 + rsi + LIsRep * LPMULT], dx

        add     r11, LLenCoder * LPMULT
        add     r8d, LkNumStates * LPMULT

# ---------- LEN DECODE ----------
Llen_decode:
        mov     r12d, 8 - 1 - LkMatchMinLen
# IF_BIT_0_NOUP probs, 0, 0, len_mid_0
        movzx   edx, word ptr [0 * 1 + r11 + 0 * LPMULT]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        cmp     ebp, eax
        jb      Llen_mid_0
# UPDATE_1 probs, 0, 0
        sub     edi, eax
        sub     ebp, eax
        mov     eax, edi
        mov     edi, edx
        shr     edx, LkNumMoveBits
        sub     edi, edx
        mov     word ptr [0 * 1 + r11 + 0 * LPMULT], di
        add     r11, (1 << (LkLenNumLowBits + LPSHIFT))
        mov     r12d, -1 - LkMatchMinLen
# IF_BIT_0_NOUP probs, 0, 0, len_mid_0
        movzx   edx, word ptr [0 * 1 + r11 + 0 * LPMULT]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        cmp     ebp, eax
        jb      Llen_mid_0
# UPDATE_1 probs, 0, 0
        sub     edi, eax
        sub     ebp, eax
        mov     eax, edi
        mov     edi, edx
        shr     edx, LkNumMoveBits
        sub     edi, edx
        mov     word ptr [0 * 1 + r11 + 0 * LPMULT], di
        add     r11, LLenHigh * LPMULT - (1 << (LkLenNumLowBits + LPSHIFT))
        mov     ebx, 1
# PLOAD   x1, probs + 1 * PMULT
        movzx   ecx, word ptr [r11 + 1 * LPMULT]

        .p2align 5
Llen8_loop:
# BIT_1   x1, x2
        movzx   edx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  edx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di
        mov     ecx, edx
        cmp     ebx, 64
        jb      Llen8_loop

        mov     r12d, (LkLenNumHighSymbols - LkLenNumLowSymbols * 2) - 1 - LkMatchMinLen
        jmp     Llen_mid_2 # we use short here for MASM that doesn't optimize that code as another assembler programs

        .p2align 5
Llen_mid_0:
# UPDATE_0 probs, 0, 0
        mov     edi, r10d
        sub     edi, edx
        shr     edi, LkNumMoveBits
        add     edx, edi
        mov     word ptr [0 * 1 + r11 + 0 * LPMULT], dx
        add     r11, rcx
# BIT_0   x2, x1
        movzx   edx, word ptr [r11 + 1 * LPMULT]
        movzx   ecx, word ptr [r11 + 1 * LPMULT_2]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + 1 * LPMULT_2 + LPMULT]
        cmovae  ecx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        cmovb   edi, r10d
        mov     ebx, 2
        sbb     ebx, 0 - 1
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [r11 + 1 * LPMULT], di
Llen_mid_2:
# BIT_1   x1, x2
        movzx   edx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  edx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di
# BIT_2   x2, len_temp
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, r12d
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di
        mov     r11, qword ptr [rsp + LCLzmaDec_Asm_Loc_probs_Spec]
        cmp     r8d, LkNumStates * LPMULT
        jb      Lcopy_match

# ---------- DECODE DISTANCE ----------
# probs + PosSlot + ((len < kNumLenToPosStates ? len : kNumLenToPosStates - 1) << kNumPosSlotBits);

        mov     edi, 3 + LkMatchMinLen
        cmp     ebx, 3 + LkMatchMinLen
        cmovb   edi, ebx
        add     r11, LPosSlot * LPMULT - (LkMatchMinLen << (LkNumPosSlotBits + LPSHIFT))
        shl     edi, (LkNumPosSlotBits + LPSHIFT)
        add     r11, rdi

# sym = Len
# mov     LOC remainLen, sym
        mov     r12d, ebx

# BIT_0   x1, x2
        movzx   ecx, word ptr [r11 + 1 * LPMULT]
        movzx   edx, word ptr [r11 + 1 * LPMULT_2]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + 1 * LPMULT_2 + LPMULT]
        cmovae  edx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        cmovb   edi, r10d
        mov     ebx, 2
        sbb     ebx, 0 - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [r11 + 1 * LPMULT], di
# BIT_1   x2, x1
        movzx   ecx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  ecx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di
# BIT_1   x1, x2
        movzx   edx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  edx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di
# BIT_1   x2, x1
        movzx   ecx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  ecx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di
# BIT_1   x1, x2
        movzx   edx, word ptr [r11 + rbx * LPMULT_2]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + rbx * LPMULT + LPMULT]
        cmovae  edx, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 0 - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di

        mov     ecx, ebx
# BIT_2   x2, 64-1
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 64-1
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [r11 + rsi * LPMULT_HALF], di

        and     ebx, 3
        mov     r11, qword ptr [rsp + LCLzmaDec_Asm_Loc_probs_Spec]
        cmp     ecx, 32 + LkEndPosModelIndex / 2
        jb      Lshort_dist

#  unsigned numDirectBits = (unsigned)(((distance >> 1) - 1));
        sub     ecx, (32 + 1 + LkNumAlignBits)
#  distance = (2 | (distance & 1));
        or      ebx, 2
# PLOAD   x2, probs + 1 * PMULT
        movzx   edx, word ptr [r11 + 1 * LPMULT]
        shl     ebx, LkNumAlignBits + 1
        lea     r9, [r11 + 2 * LPMULT]

        jmp     Ldirect_norm
# lea     t1, [sym_R + (1 SHL kNumAlignBits)]
# cmp     range, kTopValue
# jb      direct_norm

# ---------- DIRECT DISTANCE ----------
        .p2align 5
Ldirect_loop:
        shr     eax, 1
        mov     edi, ebp
        sub     ebp, eax
        cmovs   ebp, edi
        cmovns  ebx, esi

        dec     ecx
        je      Ldirect_end

        add     ebx, ebx
Ldirect_norm:
        lea     esi, [rbx + (1 << LkNumAlignBits)]
        cmp     eax, LkTopValue
        jae     Ldirect_loop
# we align for 32 here with "near ptr" command above
# NORM_2
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
        jmp     Ldirect_loop

        .p2align 5
Ldirect_end:
#  prob =  + kAlign;
#  distance <<= kNumAlignBits;
# REV_0   x2, x1
        movzx   ecx, word ptr [r9]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r11 + 3 * LPMULT]
        cmovae  ecx, edi
        cmovb   ebp, esi
        mov     edi, LkBitModelOffset
        cmovb   edi, r10d
        lea     rsi, [r11 + 3 * LPMULT]
        cmovae  r9, rsi
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [r11 + 1 * LPMULT], di
# REV_1   x1, x2, 2
        add     r9, 2 * LPMULT
        movzx   edx, word ptr [r9]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r9 + 2 * LPMULT]
        cmovae  edx, edi
        cmovb   ebp, esi
        mov     edi, LkBitModelOffset
        cmovb   edi, r10d
        lea     rsi, [r9 + 2 * LPMULT]
        cmovae  r9, rsi
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [rsi - 2 * LPMULT_2], di
# REV_1   x2, x1, 4
        add     r9, 4 * LPMULT
        movzx   ecx, word ptr [r9]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        movzx   edi, word ptr [r9 + 4 * LPMULT]
        cmovae  ecx, edi
        cmovb   ebp, esi
        mov     edi, LkBitModelOffset
        cmovb   edi, r10d
        lea     rsi, [r9 + 4 * LPMULT]
        cmovae  r9, rsi
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [rsi - 4 * LPMULT_2], di
# REV_2   x1, 8
        sub     r9, r11
        shr     r9d, LPSHIFT
        or      ebx, r9d
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        lea     edi, [ebx - 8]
        cmovb   ebx, edi
        cmovb   ebp, esi
        mov     edi, LkBitModelOffset
        cmovb   edi, r10d
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [r11 + r9 * LPMULT], di

Ldecode_dist_end:

# if (distance >= (checkDicSize == 0 ? processedPos: checkDicSize))

        mov     esi, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep0]
        mov     ecx, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep1]
        mov     edx, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep2]

        mov     edi, dword ptr [rsp + LCLzmaDec_Asm_Loc_checkDicSize]
        test    edi, edi
        cmove   edi, r13d
        cmp     ebx, edi
        jae     Lend_of_payload
# jmp     end_of_payload ; for debug

# rep3 = rep2;
# rep2 = rep1;
# rep1 = rep0;
# rep0 = distance + 1;

        inc     ebx
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep0], ebx
# mov     sym, LOC remainLen
        mov     ebx, r12d
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep1], esi
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep2], ecx
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep3], edx

# state = (state < kNumStates + kNumLitStates) ? kNumLitStates : kNumLitStates + 3;
        cmp     r8d, (LkNumStates + LkNumLitStates) * LPMULT
        mov     r8d, LkNumLitStates * LPMULT
        mov     edi, (LkNumLitStates + 3) * LPMULT
        cmovae  r8d, edi

# ---------- COPY MATCH ----------
Lcopy_match:

# len += kMatchMinLen;
# add     sym, kMatchMinLen

# if ((rem = limit - dicPos) == 0)
# {{
#   p->dicPos = dicPos;
#   return SZ_ERROR_DATA;
# }}
        mov     rdx, qword ptr [rsp + LCLzmaDec_Asm_Loc_limit]
        sub     rdx, r14
        jz      Lfin_dicPos_LIMIT

# curLen = ((rem < len) ? (unsigned)rem : len);
        cmp     rdx, rbx
# cmovae  cnt_R, sym_R ; 64-bit
        cmovae  edx, ebx # 32-bit

        mov     r12, qword ptr [rsp + LCLzmaDec_Asm_Loc_dic_Spec]
        mov     ecx, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep0]

        mov     rdi, r14
        add     r14, rdx
# processedPos += curLen;
        add     r13d, edx
# len -= curLen;
        sub     ebx, edx
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_remainLen], ebx

        sub     rdi, r12

# pos = dicPos - rep0 + (dicPos < rep0 ? dicBufSize : 0);
        sub     rdi, rcx
        jae     1f

        mov     rcx, qword ptr [rsp + LCLzmaDec_Asm_Loc_dicBufSize]
        add     rdi, rcx
        sub     rcx, rdi
        cmp     rdx, rcx
        ja      Lcopy_match_cross
1:
# if (curLen <= dicBufSize - pos)

# ---------- COPY MATCH FAST ----------
# Byte *dest = dic + dicPos;
# mov     r1, dic
# ptrdiff_t src = (ptrdiff_t)pos - (ptrdiff_t)dicPos;
# sub   t0_R, dicPos
# dicPos += curLen;

# const Byte *lim = dest + curLen;
        add     rdi, r12
        movzx   ebx, byte ptr[rdi]
        add     rdi, rdx
        neg     rdx
# lea     r1, [dicPos - 1]
Lcopy_common:
        dec     r14
# cmp   LOC rep0, 1
# je    rep0Label

# t0_R - src_lim
# r1 - dest_lim - 1
# cnt_R - (-cnt)

# IsMatchBranch_Pre
        mov     ecx, dword ptr [rsp + LCLzmaDec_Asm_Loc_pbMask]
        and     ecx, r13d
        shl     ecx, (LkLenNumLowBits + 1 + LPSHIFT)
        lea     rsi, [r11 + 1 * r8]
        inc     rdx
        jz      Lcopy_end
        .p2align 4
1:
        mov     byte ptr[rdx * 1 + r14], bl
        movzx   ebx, byte ptr[rdx * 1 + rdi]
        inc     rdx
        jnz     1b

Lcopy_end:
Llz_end_match:
        mov     byte ptr[r14], bl
        inc     r14

# IsMatchBranch_Pre
# CheckLimits
        cmp     r15, qword ptr [rsp + LCLzmaDec_Asm_Loc_bufLimit]
        jae     Lfin_OK
        cmp     r14, qword ptr [rsp + LCLzmaDec_Asm_Loc_limit]
        jae     Lfin_OK
Llz_end:
# IF_BIT_1_NOUP probs_state_R, pbPos_R, IsMatch, IsMatch_label
        movzx   edx, word ptr [rcx * 1 + rsi + LIsMatch * LPMULT]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        cmp     ebp, eax
        jae     LIsMatch_label

# ---------- LITERAL MATCHED ----------

# LIT_PROBS LOC lpMask
        mov     edi, r13d
        shl     edi, 8
        add     ebx, edi
        and     ebx, dword ptr [rsp + LCLzmaDec_Asm_Loc_lpMask]
        add     rsi, rcx
        mov     ecx, dword ptr [rsp + LCLzmaDec_Asm_Loc_lc2]
        lea     ebx, dword ptr[rbx + 2 * rbx]
        add     r11, LLiteral * LPMULT
        shl     ebx, cl
        add     r11, rbx
        mov     edi, r10d
        sub     edi, edx
        shr     edi, LkNumMoveBits
        add     edx, edi
        mov     word ptr [0 * 1 + rsi + LIsMatch * LPMULT], dx
        inc     r13d

# matchByte = dic[dicPos - rep0 + (dicPos < rep0 ? dicBufSize : 0)];
        mov     ecx, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep0]
# mov     dic, LOC dic_Spec
        mov     qword ptr [rsp + LCLzmaDec_Asm_Loc_dicPos_Spec], r14

# state -= (state < 10) ? 3 : 6;
        lea     edi, [r8 - 6 * LPMULT]
        sub     r8d, 3 * LPMULT
        cmp     r8d, 7 * LPMULT
        cmovae  r8d, edi

        sub     r14, r12
        sub     r14, rcx
        jae     1f
        add     r14, qword ptr [rsp + LCLzmaDec_Asm_Loc_dicBufSize]
1:

        movzx   r9d, byte ptr[r12 + r14 * 1]

# LITM_0
        mov     r12d, 256 * LPMULT
        shl     r9d, (LPSHIFT + 1)
        mov     r14d, r12d
        and     r14d, r9d
        movzx   ecx, word ptr [r11 + 256 * LPMULT + r14 * 1 + 1 * LPMULT]
        lea     rdx, [r11 + 256 * LPMULT + r14 * 1 + 1 * LPMULT]
        xor     r12d, r14d
        add     r9d, r9d
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  r12d, r14d
        mov     r14d, r9d
        cmovae  eax, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        cmovb   edi, r10d
        mov     ebx, 0
        sbb     ebx, -2-1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [rdx], di
# LITM
        and     r14d, r12d
        lea     rdx, [r11 + r12 * 1]
        add     rdx, r14
        movzx   ecx, word ptr [rdx + rbx * LPMULT]
        xor     r12d, r14d
        add     ebx, ebx
        add     r9d, r9d
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  r12d, r14d
        mov     r14d, r9d
        cmovae  eax, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [rdx + rsi * LPMULT_HALF], di
# LITM
        and     r14d, r12d
        lea     rdx, [r11 + r12 * 1]
        add     rdx, r14
        movzx   ecx, word ptr [rdx + rbx * LPMULT]
        xor     r12d, r14d
        add     ebx, ebx
        add     r9d, r9d
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  r12d, r14d
        mov     r14d, r9d
        cmovae  eax, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [rdx + rsi * LPMULT_HALF], di
# LITM
        and     r14d, r12d
        lea     rdx, [r11 + r12 * 1]
        add     rdx, r14
        movzx   ecx, word ptr [rdx + rbx * LPMULT]
        xor     r12d, r14d
        add     ebx, ebx
        add     r9d, r9d
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  r12d, r14d
        mov     r14d, r9d
        cmovae  eax, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [rdx + rsi * LPMULT_HALF], di
# LITM
        and     r14d, r12d
        lea     rdx, [r11 + r12 * 1]
        add     rdx, r14
        movzx   ecx, word ptr [rdx + rbx * LPMULT]
        xor     r12d, r14d
        add     ebx, ebx
        add     r9d, r9d
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  r12d, r14d
        mov     r14d, r9d
        cmovae  eax, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [rdx + rsi * LPMULT_HALF], di
# LITM
        and     r14d, r12d
        lea     rdx, [r11 + r12 * 1]
        add     rdx, r14
        movzx   ecx, word ptr [rdx + rbx * LPMULT]
        xor     r12d, r14d
        add     ebx, ebx
        add     r9d, r9d
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  r12d, r14d
        mov     r14d, r9d
        cmovae  eax, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [rdx + rsi * LPMULT_HALF], di
# LITM
        and     r14d, r12d
        lea     rdx, [r11 + r12 * 1]
        add     rdx, r14
        movzx   ecx, word ptr [rdx + rbx * LPMULT]
        xor     r12d, r14d
        add     ebx, ebx
        add     r9d, r9d
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  r12d, r14d
        mov     r14d, r9d
        cmovae  eax, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [rdx + rsi * LPMULT_HALF], di
# LITM_2
        and     r14d, r12d
        lea     rdx, [r11 + r12 * 1]
        add     rdx, r14
        movzx   ecx, word ptr [rdx + rbx * LPMULT]
        add     ebx, ebx
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, ecx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        mov     esi, ebx
        cmovb   edi, r10d
        sbb     ebx, 256 - 1
        sub     edi, ecx
        sar     edi, LkNumMoveBits
        add     edi, ecx
        mov     word ptr [rdx + rsi * LPMULT_HALF], di

        mov     r11, qword ptr [rsp + LCLzmaDec_Asm_Loc_probs_Spec]
# IsMatchBranch_Pre
        mov     ecx, dword ptr [rsp + LCLzmaDec_Asm_Loc_pbMask]
        and     ecx, r13d
        shl     ecx, (LkLenNumLowBits + 1 + LPSHIFT)
        lea     rsi, [r11 + 1 * r8]
# mov     dic, LOC dic_Spec
        mov     r14, qword ptr [rsp + LCLzmaDec_Asm_Loc_dicPos_Spec]
        mov     byte ptr[r14], bl
        inc     r14

# CheckLimits
        cmp     r15, qword ptr [rsp + LCLzmaDec_Asm_Loc_bufLimit]
        jae     Lfin_OK
        cmp     r14, qword ptr [rsp + LCLzmaDec_Asm_Loc_limit]
        jae     Lfin_OK
Llit_matched_end:
# IF_BIT_1_NOUP probs_state_R, pbPos_R, IsMatch, IsMatch_label
        movzx   edx, word ptr [rcx * 1 + rsi + LIsMatch * LPMULT]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        cmp     ebp, eax
        jae     LIsMatch_label
# IsMatchBranch
        mov     r9d, dword ptr [rsp + LCLzmaDec_Asm_Loc_lpMask]
        sub     r8d, 3 * LPMULT
        jmp     Llit_start_2

# ---------- REP 0 LITERAL ----------
        .p2align 5
LIsRep0Short_label:
# UPDATE_0 probs_state_R, pbPos_R, IsRep0Long
        mov     edi, r10d
        sub     edi, edx
        shr     edi, LkNumMoveBits
        add     edx, edi
        mov     word ptr [rcx * 1 + rsi + LIsRep0Long * LPMULT], dx

# dic[dicPos] = dic[dicPos - rep0 + (dicPos < rep0 ? dicBufSize : 0)];
        mov     r12, qword ptr [rsp + LCLzmaDec_Asm_Loc_dic_Spec]
        mov     rdi, r14
        mov     edx, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep0]
        sub     rdi, r12

        sub     r11, LRepLenCoder * LPMULT

# state = state < kNumLitStates ? 9 : 11;
        or      r8d, 1 * LPMULT

# the caller doesn't allow (dicPos >= limit) case for REP_SHORT
# so we don't need the following (dicPos == limit) check here:
# cmp     dicPos, LOC limit
# jae     fin_dicPos_LIMIT_REP_SHORT

        inc     r13d

# IsMatchBranch_Pre
        mov     ecx, dword ptr [rsp + LCLzmaDec_Asm_Loc_pbMask]
        and     ecx, r13d
        shl     ecx, (LkLenNumLowBits + 1 + LPSHIFT)
        lea     rsi, [r11 + 1 * r8]

#        xor     sym, sym
#        sub     t0_R, probBranch_R
#        cmovb   sym_R, LOC dicBufSize
#        add     t0_R, sym_R
        sub     rdi, rdx
        jae     1f
        add     rdi, qword ptr [rsp + LCLzmaDec_Asm_Loc_dicBufSize]
1:
        movzx   ebx, byte ptr[r12 + rdi * 1]
        jmp     Llz_end_match

        .p2align 5
LIsRep_label:
# UPDATE_1 probs_state_R, 0, IsRep
        sub     edi, eax
        sub     ebp, eax
        mov     eax, edi
        mov     edi, edx
        shr     edx, LkNumMoveBits
        sub     edi, edx
        mov     word ptr [0 * 1 + rsi + LIsRep * LPMULT], di

# The (checkDicSize == 0 && processedPos == 0) case was checked before in LzmaDec.c with kBadRepCode.
# So we don't check it here.

# mov     t0, processedPos
# or      t0, LOC checkDicSize
# jz      fin_ERROR_2

# state = state < kNumLitStates ? 8 : 11;
        cmp     r8d, LkNumLitStates * LPMULT
        mov     r8d, 8 * LPMULT
        mov     edx, 11 * LPMULT
        cmovae  r8d, edx

# prob = probs + RepLenCoder;
        add     r11, LRepLenCoder * LPMULT

# IF_BIT_1 probs_state_R, 0, IsRepG0, IsRepG0_label
        movzx   edx, word ptr [0 * 1 + rsi + LIsRepG0 * LPMULT]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        cmp     ebp, eax
        jae     LIsRepG0_label
        mov     edi, r10d
        sub     edi, edx
        shr     edi, LkNumMoveBits
        add     edx, edi
        mov     word ptr [0 * 1 + rsi + LIsRepG0 * LPMULT], dx
# IF_BIT_0_NOUP probs_state_R, pbPos_R, IsRep0Long, IsRep0Short_label
        movzx   edx, word ptr [rcx * 1 + rsi + LIsRep0Long * LPMULT]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        cmp     ebp, eax
        jb      LIsRep0Short_label
# UPDATE_1 probs_state_R, pbPos_R, IsRep0Long
        sub     edi, eax
        sub     ebp, eax
        mov     eax, edi
        mov     edi, edx
        shr     edx, LkNumMoveBits
        sub     edi, edx
        mov     word ptr [rcx * 1 + rsi + LIsRep0Long * LPMULT], di
        jmp     Llen_decode

        .p2align 5
LIsRepG0_label:
# UPDATE_1 probs_state_R, 0, IsRepG0
        sub     edi, eax
        sub     ebp, eax
        mov     eax, edi
        mov     edi, edx
        shr     edx, LkNumMoveBits
        sub     edi, edx
        mov     word ptr [0 * 1 + rsi + LIsRepG0 * LPMULT], di
        mov     r9d, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep0]
        mov     ebx, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep1]
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep1], r9d

# IF_BIT_1 probs_state_R, 0, IsRepG1, IsRepG1_label
        movzx   edx, word ptr [0 * 1 + rsi + LIsRepG1 * LPMULT]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        cmp     ebp, eax
        jae     LIsRepG1_label
        mov     edi, r10d
        sub     edi, edx
        shr     edi, LkNumMoveBits
        add     edx, edi
        mov     word ptr [0 * 1 + rsi + LIsRepG1 * LPMULT], dx
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep0], ebx
        jmp     Llen_decode

# MY_ALIGN_32
LIsRepG1_label:
# UPDATE_1 probs_state_R, 0, IsRepG1
        sub     edi, eax
        sub     ebp, eax
        mov     eax, edi
        mov     edi, edx
        shr     edx, LkNumMoveBits
        sub     edi, edx
        mov     word ptr [0 * 1 + rsi + LIsRepG1 * LPMULT], di
        mov     r9d, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep2]
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep2], ebx

# IF_BIT_1 probs_state_R, 0, IsRepG2, IsRepG2_label
        movzx   edx, word ptr [0 * 1 + rsi + LIsRepG2 * LPMULT]
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        cmp     ebp, eax
        jae     LIsRepG2_label
        mov     edi, r10d
        sub     edi, edx
        shr     edi, LkNumMoveBits
        add     edx, edi
        mov     word ptr [0 * 1 + rsi + LIsRepG2 * LPMULT], dx
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep0], r9d
        jmp     Llen_decode

# MY_ALIGN_32
LIsRepG2_label:
# UPDATE_1 probs_state_R, 0, IsRepG2
        sub     edi, eax
        sub     ebp, eax
        mov     eax, edi
        mov     edi, edx
        shr     edx, LkNumMoveBits
        sub     edi, edx
        mov     word ptr [0 * 1 + rsi + LIsRepG2 * LPMULT], di
        mov     ebx, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep3]
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep3], r9d
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep0], ebx
        jmp     Llen_decode

# ---------- SPEC SHORT DISTANCE ----------

        .p2align 5
Lshort_dist:
        sub     ecx, 32 + 1
        jbe     Ldecode_dist_end
        or      ebx, 2
        shl     ebx, cl
        lea     rbx, [r11 + rbx * LPMULT + LSpecPos * LPMULT + 1 * LPMULT]
        mov     r9d, LPMULT # step
        .p2align 5
Lspec_loop:
# REV_1_VAR x2
        movzx   edx, word ptr [rbx]
        mov     r11, rbx
        add     rbx, r9
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:
        mov     edi, eax
        shr     eax, LkNumBitModelTotalBits
        imul    eax, edx
        sub     edi, eax
        mov     esi, ebp
        sub     ebp, eax
        cmovae  eax, edi
        lea     rdi, [rbx + 1 * r9]
        cmovae  rbx, rdi
        mov     edi, LkBitModelOffset
        cmovb   ebp, esi
        cmovb   edi, r10d
        add     r9d, r9d
        sub     edi, edx
        sar     edi, LkNumMoveBits
        add     edi, edx
        mov     word ptr [r11], di
        dec     ecx
        jnz     Lspec_loop

        mov     r11, qword ptr [rsp + LCLzmaDec_Asm_Loc_probs_Spec]
        sub     ebx, r9d
        sub     ebx, LSpecPos * LPMULT
        sub     rbx, r11
        shr     ebx, LPSHIFT

        jmp     Ldecode_dist_end

# ---------- COPY MATCH CROSS ----------
Lcopy_match_cross:
# t0_R - src pos
# r1 - len to dicBufSize
# cnt_R - total copy len

        mov     rsi, rdi # srcPos
        mov     rdi, r12
        mov     rcx, qword ptr [rsp + LCLzmaDec_Asm_Loc_dicBufSize]
        neg     rdx
1:
        movzx   ebx, byte ptr[rsi * 1 + rdi]
        inc     rsi
        mov     byte ptr[rdx * 1 + r14], bl
        inc     rdx
        cmp     rsi, rcx
        jne     1b

        movzx   ebx, byte ptr[rdi]
        sub     rdi, rdx
        jmp     Lcopy_common

# fin_dicPos_LIMIT_REP_SHORT:
# mov     sym, 1

Lfin_dicPos_LIMIT:
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_remainLen], ebx
        jmp     Lfin_OK
# For more strict mode we can stop decoding with error
# mov     sym, 1
# jmp     fin

Lfin_ERROR_MATCH_DIST:

# rep3 = rep2;
# rep2 = rep1;
# rep1 = rep0;
# rep0 = distance + 1;

        add     r12d, LkMatchSpecLen_Error_Data
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_remainLen], r12d

        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep0], ebx
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep1], esi
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep2], ecx
        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_rep3], edx

# state = (state < kNumStates + kNumLitStates) ? kNumLitStates : kNumLitStates + 3;
        cmp     r8d, (LkNumStates + LkNumLitStates) * LPMULT
        mov     r8d, LkNumLitStates * LPMULT
        mov     edi, (LkNumLitStates + 3) * LPMULT
        cmovae  r8d, edi

# jmp     fin_OK
        mov     ebx, 1
        jmp     Lfin

Lend_of_payload:
        inc     ebx
        jnz     Lfin_ERROR_MATCH_DIST

        mov     dword ptr [rsp + LCLzmaDec_Asm_Loc_remainLen], LkMatchSpecLenStart
        sub     r8d, LkNumStates * LPMULT

Lfin_OK:
        xor     ebx, ebx

Lfin:
# NORM
        cmp     eax, LkTopValue
        jae     1f
        shl     ebp, 8
        mov     bpl, byte ptr [r15]
        shl     eax, 8
        inc     r15
1:

        mov     rcx, qword ptr [rsp + LCLzmaDec_Asm_Loc_lzmaPtr]

        sub     r14, qword ptr [rsp + LCLzmaDec_Asm_Loc_dic_Spec]
        mov     qword ptr [rcx + LCLzmaDec_Asm_dicPos_Spec], r14
        mov     qword ptr [rcx + LCLzmaDec_Asm_buf_Spec], r15
        mov     dword ptr [rcx + LCLzmaDec_Asm_range_Spec], eax
        mov     dword ptr [rcx + LCLzmaDec_Asm_code_Spec], ebp
        shr     r8d, LPSHIFT
        mov     dword ptr [rcx + LCLzmaDec_Asm_state_Spec], r8d
        mov     dword ptr [rcx + LCLzmaDec_Asm_processedPos_Spec], r13d

# RESTORE_VAR(remainLen)
        mov     edi, dword ptr [rsp + LCLzmaDec_Asm_Loc_remainLen]
        mov     dword ptr [rcx + LCLzmaDec_Asm_remainLen], edi
# RESTORE_VAR(rep0)
        mov     edi, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep0]
        mov     dword ptr [rcx + LCLzmaDec_Asm_rep0], edi
# RESTORE_VAR(rep1)
        mov     edi, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep1]
        mov     dword ptr [rcx + LCLzmaDec_Asm_rep1], edi
# RESTORE_VAR(rep2)
        mov     edi, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep2]
        mov     dword ptr [rcx + LCLzmaDec_Asm_rep2], edi
# RESTORE_VAR(rep3)
        mov     edi, dword ptr [rsp + LCLzmaDec_Asm_Loc_rep3]
        mov     dword ptr [rcx + LCLzmaDec_Asm_rep3], edi

        mov     eax, ebx

        mov     rsp, qword ptr [rsp + LCLzmaDec_Asm_Loc_OLD_RSP]

# MY_POP_PRESERVED_ABI_REGS
        pop     r15
        pop     r14
        pop     r13
        pop     r12
        pop     rbp
        pop     rbx
        ret
