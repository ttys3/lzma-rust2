// GENERATED FILE - do not edit. Produced by scripts/gen-lzma-dec-asm.sh from
// src/asm/upstream/LzmaDecOpt.S + 7zAsm.S (7-Zip, Igor Pavlov, public domain).
// The entry label, .globl and section header are emitted by src/lzma_dec_asm.rs.
// inputs: src/asm/upstream/LzmaDecOpt.S=25ee0f34dd5f304e src/asm/upstream/7zAsm.S=0666cf2e2da64d79
.macro p2_add reg:req, param:req
        add \reg, \reg, \param
.endm
.macro p2_sub reg:req, param:req
        sub \reg, \reg, \param
.endm
.macro p2_sub_s reg:req, param:req
        subs \reg, \reg, \param
.endm
.macro p2_and reg:req, param:req
        and \reg, \reg, \param
.endm
.macro xor reg:req, param:req
        eor \reg, \reg, \param
.endm
.macro or reg:req, param:req
        orr \reg, \reg, \param
.endm
.macro shl reg:req, param:req
        lsl \reg, \reg, \param
.endm
.macro shr reg:req, param:req
        lsr \reg, \reg, \param
.endm
.macro sar reg:req, param:req
        asr \reg, \reg, \param
.endm
.macro p1_neg reg:req
        neg \reg, \reg
.endm
.macro dec reg:req
        sub \reg, \reg, 1
.endm
.macro dec_s reg:req
        subs \reg, \reg, 1
.endm
.macro inc reg:req
        add \reg, \reg, 1
.endm
.macro inc_s reg:req
        adds \reg, \reg, 1
.endm
.macro imul reg:req, param:req
        mul \reg, \reg, \param
.endm
.macro jmp lab:req
        b \lab
.endm
.macro je lab:req
        b.eq \lab
.endm
.macro jz lab:req
        b.eq \lab
.endm
.macro jnz lab:req
        b.ne \lab
.endm
.macro jne lab:req
        b.ne \lab
.endm
.macro jb lab:req
        b.lo \lab
.endm
.macro jbe lab:req
        b.ls \lab
.endm
.macro ja lab:req
        b.hi \lab
.endm
.macro jae lab:req
        b.hs \lab
.endm
.macro cmove dest:req, srcTrue:req
        csel \dest, \srcTrue, \dest, eq
.endm
.macro cmovne dest:req, srcTrue:req
        csel \dest, \srcTrue, \dest, ne
.endm
.macro cmovs dest:req, srcTrue:req
        csel \dest, \srcTrue, \dest, mi
.endm
.macro cmovns dest:req, srcTrue:req
        csel \dest, \srcTrue, \dest, pl
.endm
.macro cmovb dest:req, srcTrue:req
        csel \dest, \srcTrue, \dest, lo
.endm
.macro cmovae dest:req, srcTrue:req
        csel \dest, \srcTrue, \dest, hs
.endm
.macro MY_ALIGN_16 macro
 .p2align 4,, (1 << 4) - 1
.endm
.macro MY_ALIGN_32 macro
        .p2align 5,, (1 << 5) - 1
.endm
.macro MY_ALIGN_64 macro
        .p2align 6,, (1 << 6) - 1
.endm
        .macro PLOAD dest:req, mem:req
                ldrh \dest, [\mem]
        .endm
        .macro PLOAD_PREINDEXED dest:req, mem:req, offset:req
                ldrh \dest, [\mem, \offset]!
        .endm
        .macro PLOAD_2 dest:req, mem1:req, mem2:req
                ldrh \dest, [\mem1, \mem2]
        .endm
        .macro PLOAD_LSL dest:req, mem1:req, mem2:req
                ldrh \dest, [\mem1, \mem2, lsl #1]
        .endm
        .macro PSTORE src:req, mem:req
                strh \src, [\mem]
        .endm
        .macro PSTORE_2 src:req, mem1:req, mem2:req
                strh \src, [\mem1, \mem2]
        .endm
        .macro PSTORE_LSL src:req, mem1:req, mem2:req
                strh \src, [\mem1, \mem2, lsl #1]
        .endm
        .macro PSTORE_LSL_M1 src:req, mem1:req, mem2:req, temp_reg:req
                strh \src, [\mem1, \mem2]
        .endm
.equ LPMULT , (1 << 1)
.equ LPMULT_2 , (2 << 1)
.equ LkMatchSpecLen_Error_Data , (1 << 9)
.equ LkNumBitModelTotalBits , 11
.equ LkBitModelTotal , (1 << LkNumBitModelTotalBits)
.equ LkNumMoveBits , 5
.equ LkBitModelOffset , (LkBitModelTotal - (1 << LkNumMoveBits) + 1)
.macro NORM_2 macro
        ldrb w7, [x15], 1
        shl w0, 8
        orr w5, w7, w5, lsl 8
.endm
.macro TEST_HIGH_BYTE_range macro
        tst w0, 0xFF000000
.endm
.macro NORM macro
        TEST_HIGH_BYTE_range
        jnz 1f
        NORM_2
1:
.endm
.macro UPDATE_0__0
        sub w7, w2, LkBitModelOffset
.endm
.macro UPDATE_0__1
        sub w2, w2, w7, asr #(LkNumMoveBits)
.endm
.macro UPDATE_0__2 probsArray:req, probOffset:req, probDisp:req
     .if \probDisp == 0
        PSTORE_2 w2, \probsArray, \probOffset
    .elseif \probOffset == 0
        PSTORE_2 w2, \probsArray, \probDisp * LPMULT
    .else
        .error "unsupported"
        PSTORE_2 w2, x4, \probDisp * LPMULT
    .endif
.endm
.macro UPDATE_0 probsArray:req, probOffset:req, probDisp:req
        UPDATE_0__0
        UPDATE_0__1
        UPDATE_0__2 \probsArray, \probOffset, \probDisp
.endm
.macro UPDATE_1 probsArray:req, probOffset:req, probDisp:req
        p2_sub w5, w0
        sub w0, w7, w0
        sub w7, w2, w2, lsr #(LkNumMoveBits)
    .if \probDisp == 0
        PSTORE_2 w7, \probsArray, \probOffset
    .elseif \probOffset == 0
        PSTORE_2 w7, \probsArray, \probDisp * LPMULT
    .else
        .error "unsupported"
        PSTORE_2 w7, x4, \probDisp * LPMULT
    .endif
.endm
.macro CMP_COD_BASE
        NORM
        mov w7, w0
        shr w0, LkNumBitModelTotalBits
        imul w0, w2
        cmp w5, w0
.endm
.macro CMP_COD_1 probsArray:req
        PLOAD w2, \probsArray
        CMP_COD_BASE
.endm
.macro CMP_COD_3 probsArray:req, probOffset:req, probDisp:req
    .if \probDisp == 0
        PLOAD_2 w2, \probsArray, \probOffset
    .elseif \probOffset == 0
        PLOAD_2 w2, \probsArray, \probDisp * LPMULT
    .else
        .error "unsupported"
        add x4, \probsArray, \probOffset
        PLOAD_2 w2, x4, \probDisp * LPMULT
    .endif
        CMP_COD_BASE
.endm
.macro IF_BIT_1_NOUP probsArray:req, probOffset:req, probDisp:req, toLabel:req
        CMP_COD_3 \probsArray, \probOffset, \probDisp
        jae \toLabel
.endm
.macro IF_BIT_1 probsArray:req, probOffset:req, probDisp:req, toLabel:req
        IF_BIT_1_NOUP \probsArray, \probOffset, \probDisp, \toLabel
        UPDATE_0 \probsArray, \probOffset, \probDisp
.endm
.macro IF_BIT_0_NOUP probsArray:req, probOffset:req, probDisp:req, toLabel:req
        CMP_COD_3 \probsArray, \probOffset, \probDisp
        jb \toLabel
.endm
.macro IF_BIT_0_NOUP_1 probsArray:req, toLabel:req
        CMP_COD_1 \probsArray
        jb \toLabel
.endm
.macro NORM_LSR
        NORM
        lsr w7, w0, #LkNumBitModelTotalBits
.endm
.macro COD_RANGE_SUB
        subs w6, w5, w7
        p2_sub w0, w7
.endm
.macro RANGE_IMUL prob:req
        imul w7, \prob
.endm
.macro NORM_CALC prob:req
        NORM_LSR
        RANGE_IMUL \prob
        COD_RANGE_SUB
.endm
.macro CMOV_range
        cmovb w0, w7
.endm
.macro CMOV_code
        cmovae w5, w6
.endm
.macro CMOV_code_Model_Pre prob:req
        sub w7, \prob, LkBitModelOffset
        CMOV_code
        cmovae w7, \prob
.endm
.macro PUP_BASE_2 prob:req, dest_reg:req
        sub \dest_reg, \prob, \dest_reg, asr #(LkNumMoveBits)
.endm
.macro PUP prob:req, probPtr:req, mem2:req
        PUP_BASE_2 \prob, w7
        PSTORE_2 w7, \probPtr, \mem2
.endm
.macro BIT_01
        add x10, x11, LPMULT
.endm
.macro BIT_0_R prob:req
        PLOAD_2 \prob, x11, 1 * LPMULT
        NORM_LSR
            sub w4, \prob, LkBitModelOffset
        RANGE_IMUL \prob
            PLOAD_2 w8, x11, 1 * LPMULT_2
        COD_RANGE_SUB
        CMOV_range
            cmovae w4, \prob
        PLOAD_2 w7, x11, 1 * LPMULT_2 + LPMULT
            PUP_BASE_2 \prob, w4
        csel \prob, w8, w7, lo
            CMOV_code
        mov w3, 2
        PSTORE_2 w4, x11, 1 * LPMULT
            adc w3, w3, wzr
        BIT_01
.endm
.macro BIT_1_R prob:req
        NORM_LSR
            p2_add w3, w3
            sub w4, \prob, LkBitModelOffset
        RANGE_IMUL \prob
            PLOAD_LSL w8, x11, x3
        COD_RANGE_SUB
        CMOV_range
            cmovae w4, \prob
        PLOAD_LSL w7, x10, x3
            PUP_BASE_2 \prob, w4
        csel \prob, w8, w7, lo
            CMOV_code
        PSTORE_LSL_M1 w4, x11, x3, x8
            adc w3, w3, wzr
.endm
.macro BIT_2_R prob:req
        NORM_LSR
            p2_add w3, w3
            sub w4, \prob, LkBitModelOffset
        RANGE_IMUL \prob
        COD_RANGE_SUB
        CMOV_range
            cmovae w4, \prob
            CMOV_code
            PUP_BASE_2 \prob, w4
        PSTORE_LSL_M1 w4, x11, x3, x8
            adc w3, w3, wzr
.endm
.macro LITM_0 macro
        shl w9, (1 + 1)
        and w4, w9, 256 * LPMULT
        add x2, x11, 256 * LPMULT + 1 * LPMULT
        p2_add w9, w9
        p2_add x2, x4
        eor w10, w4, 256 * LPMULT
        PLOAD w1, x2
        NORM_LSR
            sub w8, w1, LkBitModelOffset
        RANGE_IMUL w1
        COD_RANGE_SUB
        cmovae w10, w4
            CMOV_range
        and w4, w9, w10
            cmovae w8, w1
            CMOV_code
            mov w3, 2
        PUP_BASE_2 w1, w8
        PSTORE w8, x2
        add x2, x11, x10
        adc w3, w3, wzr
.endm
.macro LITM macro
        p2_add x2, x4
            xor w10, w4
        PLOAD_LSL w1, x2, x3
        NORM_LSR
            p2_add w9, w9
            sub w8, w1, LkBitModelOffset
        RANGE_IMUL w1
        COD_RANGE_SUB
        cmovae w10, w4
            CMOV_range
        and w4, w9, w10
            cmovae w8, w1
            CMOV_code
        PUP_BASE_2 w1, w8
        PSTORE_LSL w8, x2, x3
        add x2, x11, x10
        adc w3, w3, w3
.endm
.macro LITM_2 macro
        p2_add x2, x4
        PLOAD_LSL w1, x2, x3
        NORM_LSR
            sub w8, w1, LkBitModelOffset
        RANGE_IMUL w1
        COD_RANGE_SUB
            CMOV_range
            cmovae w8, w1
            CMOV_code
        PUP_BASE_2 w1, w8
        PSTORE_LSL w8, x2, x3
        adc w3, w3, w3
.endm
.macro REV_0 prob:req
        NORM_CALC \prob
        CMOV_range
        PLOAD w8, x9
        PLOAD_2 w4, x11, 3 * LPMULT
        CMOV_code_Model_Pre \prob
        add x6, x11, 3 * LPMULT
        cmovae x9, x6
        PUP \prob, x11, 1 * LPMULT
        csel \prob, w8, w4, lo
.endm
.macro REV_1 prob:req, step:req
        NORM_LSR
            PLOAD_PREINDEXED w8, x9, (\step * LPMULT)
        RANGE_IMUL \prob
        COD_RANGE_SUB
        CMOV_range
        PLOAD_2 w4, x9, (\step * LPMULT)
        sub w7, \prob, LkBitModelOffset
        CMOV_code
        add x6, x9, \step * LPMULT
        cmovae w7, \prob
        cmovae x9, x6
        PUP_BASE_2 \prob, w7
        csel \prob, w8, w4, lo
        PSTORE_2 w7, x6, 0 - \step * LPMULT_2
.endm
.macro REV_2 prob:req, step:req
        sub x6, x9, x11
        NORM_LSR
            orr w3, w3, w6, lsr #1
        RANGE_IMUL \prob
        COD_RANGE_SUB
        sub w8, w3, \step
        CMOV_range
        cmovb w3, w8
        CMOV_code_Model_Pre \prob
        PUP \prob, x9, 0
.endm
.macro REV_1_VAR prob:req
        PLOAD \prob, x3
        mov x11, x3
        p2_add x3, x9
        NORM_LSR
            add x8, x3, x9
        RANGE_IMUL \prob
        COD_RANGE_SUB
        cmovae x3, x8
        CMOV_range
        CMOV_code_Model_Pre \prob
        p2_add w9, w9
        PUP \prob, x11, 0
.endm
.macro add_big dest:req, src:req, param:req
    .if (\param) < (1 << 12)
        add \dest, \src, \param
    .else
          .error "unexpcted add_big expansion"
        add \dest, \src, (\param) / 2
        add \dest, \dest, (\param) - (\param) / 2
    .endif
.endm
.macro sub_big dest:req, src:req, param:req
    .if (\param) < (1 << 12)
        sub \dest, \src, \param
    .else
          .error "unexpcted sub_big expansion"
        sub \dest, \src, (\param) / 2
        sub \dest, \dest, (\param) - (\param) / 2
    .endif
.endm
.macro SET_probs offset:req
        add x11, x25, ((\offset) - LIsMatch) * LPMULT
.endm
.macro LIT_PROBS
        add w3, w3, w28, lsl 8
        inc w28
        UPDATE_0__0
        shl w3, w30
        SET_probs LLiteral
        p2_and w3, w30
        p2_add x11, x3
        UPDATE_0__1
        add x11, x11, x3, lsl 1
        UPDATE_0__2 x6, x1, 0
.endm
.equ LkNumPosBitsMax , 4
.equ LkNumPosStatesMax , (1 << LkNumPosBitsMax)
.equ LkLenNumLowBits , 3
.equ LkLenNumLowSymbols , (1 << LkLenNumLowBits)
.equ LkLenNumHighBits , 8
.equ LkLenNumHighSymbols , (1 << LkLenNumHighBits)
.equ LkNumLenProbs , (2 * LkLenNumLowSymbols * LkNumPosStatesMax + LkLenNumHighSymbols)
.equ LLenLow , 0
.equ LLenChoice , LLenLow
.equ LLenChoice2 , (LLenLow + LkLenNumLowSymbols)
.equ LLenHigh , (LLenLow + 2 * LkLenNumLowSymbols * LkNumPosStatesMax)
.equ LkNumStates , 12
.equ LkNumStates2 , 16
.equ LkNumLitStates , 7
.equ LkStartPosModelIndex , 4
.equ LkEndPosModelIndex , 14
.equ LkNumFullDistances , (1 << (LkEndPosModelIndex >> 1))
.equ LkNumPosSlotBits , 6
.equ LkNumLenToPosStates , 4
.equ LkNumAlignBits , 4
.equ LkAlignTableSize , (1 << LkNumAlignBits)
.equ LkMatchMinLen , 2
.equ LkMatchSpecLenStart , (LkMatchMinLen + LkLenNumLowSymbols * 2 + LkLenNumHighSymbols)
.equ LkStartOffset , 0
.equ LSpecPos , (-LkStartOffset)
.equ LIsRep0Long , (LSpecPos + LkNumFullDistances)
.equ LRepLenCoder , (LIsRep0Long + (LkNumStates2 << LkNumPosBitsMax))
.equ LLenCoder , (LRepLenCoder + LkNumLenProbs)
.equ LIsMatch , (LLenCoder + LkNumLenProbs)
.equ LkAlign , (LIsMatch + (LkNumStates2 << LkNumPosBitsMax))
.equ LIsRep , (LkAlign + LkAlignTableSize)
.equ LIsRepG0 , (LIsRep + LkNumStates)
.equ LIsRepG1 , (LIsRepG0 + LkNumStates)
.equ LIsRepG2 , (LIsRepG1 + LkNumStates)
.equ LPosSlot , (LIsRepG2 + LkNumStates)
.equ LLiteral , (LPosSlot + (LkNumLenToPosStates << LkNumPosSlotBits))
.equ LNUM_BASE_PROBS , (LLiteral + LkStartOffset)
.if LkStartOffset != 0
  .error "Stop_Compiling_Bad_StartOffset"
.endif
.if LNUM_BASE_PROBS != 1984
  .error "Stop_Compiling_Bad_LZMA_PROBS"
.endif
.equ Loffset_lc , 0
.equ Loffset_lp , 1
.equ Loffset_pb , 2
.equ Loffset_dicSize , 4
.equ Loffset_probs , 4 + Loffset_dicSize
.equ Loffset_probs_1664 , 8 + Loffset_probs
.equ Loffset_dic , 8 + Loffset_probs_1664
.equ Loffset_dicBufSize , 8 + Loffset_dic
.equ Loffset_dicPos , 8 + Loffset_dicBufSize
.equ Loffset_buf , 8 + Loffset_dicPos
.equ Loffset_range , 8 + Loffset_buf
.equ Loffset_code , 4 + Loffset_range
.equ Loffset_processedPos , 4 + Loffset_code
.equ Loffset_checkDicSize , 4 + Loffset_processedPos
.equ Loffset_rep0 , 4 + Loffset_checkDicSize
.equ Loffset_rep1 , 4 + Loffset_rep0
.equ Loffset_rep2 , 4 + Loffset_rep1
.equ Loffset_rep3 , 4 + Loffset_rep2
.equ Loffset_state , 4 + Loffset_rep3
.equ Loffset_remainLen , 4 + Loffset_state
.equ Loffset_TOTAL_SIZE , 4 + Loffset_remainLen
.if Loffset_TOTAL_SIZE != 96
  .error "Incorrect Loffset_TOTAL_SIZE"
.endif
.macro IsMatchBranch_Pre
        and w1, w29, w28, lsl #(LkLenNumLowBits + 1 + 1)
        add x6, x25, x13
.endm
.macro CheckLimits
        cmp x15, x16
        jae Lfin_OK
        cmp x14, x19
        jae Lfin_OK
.endm
.macro LOAD_LZMA_VAR reg:req, struct_offs:req
        ldr \reg, [x0, \struct_offs]
.endm
.macro LOAD_LZMA_BYTE reg:req, struct_offs:req
        ldrb \reg, [x0, \struct_offs]
.endm
.macro LOAD_LZMA_PAIR reg0:req, reg1:req, struct_offs:req
        ldp \reg0, \reg1, [x0, \struct_offs]
.endm
 stp x19, x20, [sp, -128]!
 stp x21, x22, [sp, 16]
 stp x23, x24, [sp, 32]
 stp x25, x26, [sp, 48]
 stp x27, x28, [sp, 64]
 stp x29, x30, [sp, 80]
        str x0, [sp, 120]
        mov x16, x2
        mov x19, x1
        LOAD_LZMA_PAIR x24, x17, Loffset_dic
        LOAD_LZMA_PAIR x14, x15, Loffset_dicPos
        LOAD_LZMA_PAIR w20, w21, Loffset_rep0
        LOAD_LZMA_PAIR w22, w23, Loffset_rep2
        mov w7, 1 << (LkLenNumLowBits + 1 + 1)
        LOAD_LZMA_BYTE w29, Loffset_pb
        p2_add x19, x24
        mov w12, wzr
        lsl w29, w7, w29
        p2_add x14, x24
        p2_sub w29, w7
        LOAD_LZMA_BYTE w30, Loffset_lc
        mov w7, 256 << 1
        LOAD_LZMA_BYTE w6, Loffset_lp
        p2_add w6, w30
        p2_sub w30, (256 << 1) - 1
        shl w7, w6
        p2_add w30, w7
        LOAD_LZMA_VAR x26, Loffset_probs
        LOAD_LZMA_VAR w27, Loffset_checkDicSize
        LOAD_LZMA_VAR w28, Loffset_processedPos
        LOAD_LZMA_VAR w13, Loffset_state
        LOAD_LZMA_PAIR w0, w5, Loffset_range
        mov w3, wzr
        shl w13, 1
        add_big x25, x26, ((LIsMatch - LSpecPos) << 1)
        orr w7, w27, w28
        cbz w7, 1f
        add x7, x17, x24
        cmp x14, x24
        cmovne x7, x14
        ldrb w3, [x7, -1]
1:
        IsMatchBranch_Pre
        cmp w13, 4 * LPMULT
        jb Llit_end
        cmp w13, LkNumLitStates * LPMULT
        jb Llit_matched_end
        jmp Llz_end
MY_ALIGN_64
Llit_start:
        mov w13, wzr
Llit_start_2:
        LIT_PROBS
        BIT_0_R w1
        BIT_1_R w1
        BIT_1_R w1
        BIT_1_R w1
        BIT_1_R w1
        BIT_1_R w1
        BIT_1_R w1
        BIT_2_R w1
        IsMatchBranch_Pre
        strb w3, [x14], 1
        p2_and w3, 255
        CheckLimits
Llit_end:
        IF_BIT_0_NOUP x6, x1, (LIsMatch - LIsMatch), Llit_start
LIsMatch_label:
        UPDATE_1 x6, x1, (LIsMatch - LIsMatch)
        IF_BIT_1 x6, 0, (LIsRep - LIsMatch), LIsRep_label
        SET_probs LLenCoder
        or w13, (1 << (4 + 1))
Llen_decode:
        mov w12, 8 - LkMatchMinLen
        IF_BIT_0_NOUP_1 x11, Llen_mid_0
        UPDATE_1 x11, 0, 0
        p2_add x11, (1 << (LkLenNumLowBits + 1))
        mov w12, 0 - LkMatchMinLen
        IF_BIT_0_NOUP_1 x11, Llen_mid_0
        UPDATE_1 x11, 0, 0
        p2_add x11, LLenHigh * LPMULT - (1 << (LkLenNumLowBits + 1))
        PLOAD_2 w1, x11, 1 * LPMULT
        mov w3, 1
        BIT_01
MY_ALIGN_32
Llen8_loop:
        BIT_1_R w1
        tbz w3, 6, Llen8_loop
        mov w12, (LkLenNumHighSymbols - LkLenNumLowSymbols * 2) - LkMatchMinLen
        jmp Llen_mid_2
MY_ALIGN_32
Llen_mid_0:
        UPDATE_0 x11, 0, 0
        p2_add x11, x1
        BIT_0_R w1
Llen_mid_2:
        BIT_1_R w1
        BIT_2_R w1
        sub w12, w3, w12
        tbz w13, (4 + 1), Lcopy_match
        mov w7, 3 + LkMatchMinLen
        cmp w12, 3 + LkMatchMinLen
        cmovb w7, w12
        SET_probs LPosSlot - (LkMatchMinLen << (LkNumPosSlotBits))
        add x11, x11, x7, lsl #(LkNumPosSlotBits + 1)
        BIT_0_R w1
        BIT_1_R w1
        BIT_1_R w1
        BIT_1_R w1
        BIT_1_R w1
        mov w10, w3
        BIT_2_R w1
        p2_and w3, 3
        cmp w10, 32 + LkEndPosModelIndex / 2
        jb Lshort_dist
        SET_probs LkAlign
        p2_sub w10, (32 + 1 + LkNumAlignBits)
        or w3, 2
        PLOAD_2 w1, x11, 1 * LPMULT
        add x9, x11, 2 * LPMULT
.macro DIRECT_1
        shr w0, 1
        subs w7, w5, w0
        p2_add w3, w3
        csel w5, w5, w7, mi
        csinc w3, w3, w3, mi
        dec_s w10
        je Ldirect_end
.endm
.macro DIRECT_2
        TEST_HIGH_BYTE_range
        jz Ldirect_unroll
        DIRECT_1
.endm
        DIRECT_2
        DIRECT_2
        DIRECT_2
        DIRECT_2
        DIRECT_2
        DIRECT_2
        DIRECT_2
        DIRECT_2
Ldirect_unroll:
        NORM_2
        DIRECT_1
        DIRECT_1
        DIRECT_1
        DIRECT_1
        DIRECT_1
        DIRECT_1
        DIRECT_1
        DIRECT_1
        jmp Ldirect_unroll
MY_ALIGN_32
Ldirect_end:
        shl w3, LkNumAlignBits
        REV_0 w1
        REV_1 w1, 2
        REV_1 w1, 4
        REV_2 w1, 8
Ldecode_dist_end:
        tst w27, w27
        csel w7, w28, w27, eq
        cmp w3, w7
        jae Lend_of_payload
        mov w23, w22
        mov w22, w21
        mov w21, w20
        add w20, w3, 1
.macro STATE_UPDATE_FOR_MATCH
        cmp w13, LkNumLitStates * LPMULT + (1 << (4 + 1))
        mov w13, LkNumLitStates * LPMULT
        mov w7, (LkNumLitStates + 3) * LPMULT
        cmovae w13, w7
.endm
        STATE_UPDATE_FOR_MATCH
Lcopy_match:
        subs x2, x19, x14
        jz Lfin_OK
        cmp x2, x12
        cmovae w2, w12
        sub x7, x14, x24
        p2_add x14, x2
        p2_add w28, w2
        p2_sub w12, w2
        p2_sub_s x7, x20
        jae 1f
        cmn x7, x2
        p2_add x7, x17
        ja Lcopy_match_cross
1:
        p2_add x7, x24
        ldrb w3, [x7]
        p2_add x7, x2
        p1_neg x2
Lcopy_common:
        dec x14
        IsMatchBranch_Pre
        inc_s x2
        jz Lcopy_end
        cmp w20, 1
        je Lcopy_match_0
MY_ALIGN_32
1:
        strb w3, [x14, x2]
        ldrb w3, [x7, x2]
        inc_s x2
        jz Lcopy_end
        strb w3, [x14, x2]
        ldrb w3, [x7, x2]
        inc_s x2
        jnz 1b
Lcopy_end:
Llz_end_match:
        strb w3, [x14], 1
        CheckLimits
Llz_end:
        IF_BIT_1_NOUP x6, x1, (LIsMatch - LIsMatch), LIsMatch_label
        LIT_PROBS
        sub x7, x14, x24
        p2_sub_s x7, x20
        jae 1f
        p2_add x7, x17
1:
        ldrb w9, [x24, x7]
        sub w3, w13, 6 * LPMULT
        cmp w13, 10 * LPMULT
        p2_sub w13, 3 * LPMULT
        cmovae w13, w3
        LITM_0
        LITM
        LITM
        LITM
        LITM
        LITM
        LITM
        LITM_2
        IsMatchBranch_Pre
        strb w3, [x14], 1
        p2_and w3, 255
        CheckLimits
Llit_matched_end:
        IF_BIT_1_NOUP x6, x1, (LIsMatch - LIsMatch), LIsMatch_label
        p2_sub w13, 3 * LPMULT
        jmp Llit_start_2
MY_ALIGN_32
LIsRep0Short_label:
        UPDATE_0 x6, x1, 0
        sub x7, x14, x24
        or w13, 1 * LPMULT
        inc w28
        IsMatchBranch_Pre
        p2_sub_s x7, x20
        jae 1f
        p2_add x7, x17
1:
        ldrb w3, [x24, x7]
        jmp Llz_end_match
MY_ALIGN_32
LIsRep_label:
        UPDATE_1 x6, 0, (LIsRep - LIsMatch)
        cmp w13, LkNumLitStates * LPMULT
        mov w13, 8 * LPMULT
        mov w2, 11 * LPMULT
        cmovae w13, w2
        SET_probs LRepLenCoder
        IF_BIT_1 x6, 0, (LIsRepG0 - LIsMatch), LIsRepG0_label
        sub_big x6, x6, (LIsMatch - LIsRep0Long) << 1
        IF_BIT_0_NOUP x6, x1, 0, LIsRep0Short_label
        UPDATE_1 x6, x1, 0
        jmp Llen_decode
MY_ALIGN_32
LIsRepG0_label:
        UPDATE_1 x6, 0, (LIsRepG0 - LIsMatch)
        IF_BIT_1 x6, 0, (LIsRepG1 - LIsMatch), LIsRepG1_label
        mov w3, w21
        mov w21, w20
        mov w20, w3
        jmp Llen_decode
LIsRepG1_label:
        UPDATE_1 x6, 0, (LIsRepG1 - LIsMatch)
        IF_BIT_1 x6, 0, (LIsRepG2 - LIsMatch), LIsRepG2_label
        mov w3, w22
        mov w22, w21
        mov w21, w20
        mov w20, w3
        jmp Llen_decode
LIsRepG2_label:
        UPDATE_1 x6, 0, (LIsRepG2 - LIsMatch)
        mov w3, w23
        mov w23, w22
        mov w22, w21
        mov w21, w20
        mov w20, w3
        jmp Llen_decode
MY_ALIGN_32
Lshort_dist:
        p2_sub_s w10, 32 + 1
        jbe Ldecode_dist_end
        or w3, 2
        shl w3, w10
        add x3, x26, x3, lsl #1
        p2_add x3, LSpecPos * LPMULT + 1 * LPMULT
        mov w9, LPMULT
MY_ALIGN_32
Lspec_loop:
        REV_1_VAR w1
        dec_s w10
        jnz Lspec_loop
        p2_add x9, x26
    .if LSpecPos != 0
        p2_add x9, LSpecPos * LPMULT
    .endif
        p2_sub x3, x9
        shr w3, 1
        jmp Ldecode_dist_end
MY_ALIGN_32
Lcopy_match_0:
        strb w3, [x14, x2]
        inc_s x2
        jz Lcopy_end
        strb w3, [x14, x2]
        inc_s x2
        jz Lcopy_end
        strb w3, [x14, x2]
        inc_s x2
        jz Lcopy_end
        orr w4, w3, w3, lsl 8
        p2_and x2, -4
        orr w4, w4, w4, lsl 16
MY_ALIGN_16
1:
        str w4, [x14, x2]
        adds x2, x2, 4
        jnz 1b
2:
    jmp Lcopy_end
Lcopy_match_cross:
        p1_neg x2
1:
        ldrb w3, [x24, x7]
        inc x7
        strb w3, [x14, x2]
        inc x2
        cmp x7, x17
        jne 1b
        ldrb w3, [x24]
        sub x7, x24, x2
        jmp Lcopy_common
Lfin_ERROR_MATCH_DIST:
        p2_add w12, LkMatchSpecLen_Error_Data
        mov w23, w22
        mov w22, w21
        mov w21, w20
        mov w20, w3
        STATE_UPDATE_FOR_MATCH
        mov w3, 1
        jmp Lfin
Lend_of_payload:
        inc_s w3
        jnz Lfin_ERROR_MATCH_DIST
        mov w12, LkMatchSpecLenStart
        xor w13, (1 << (4 + 1))
        jmp Lfin_OK
Lfin_OK:
        mov w3, wzr
Lfin:
        NORM
   .macro STORE_LZMA_VAR reg:req, struct_offs:req
        str \reg, [x7, \struct_offs]
   .endm
   .macro STORE_LZMA_PAIR reg0:req, reg1:req, struct_offs:req
        stp \reg0, \reg1, [x7, \struct_offs]
   .endm
        ldr x7, [sp, 120]
        p2_sub x14, x24
        shr w13, 1
        STORE_LZMA_PAIR x14, x15, Loffset_dicPos
        STORE_LZMA_PAIR w0, w5, Loffset_range
        STORE_LZMA_VAR w28, Loffset_processedPos
        STORE_LZMA_PAIR w20, w21, Loffset_rep0
        STORE_LZMA_PAIR w22, w23, Loffset_rep2
        STORE_LZMA_PAIR w13, w12, Loffset_state
        mov w0, w3
 ldp x29, x30, [sp, 80]
 ldp x27, x28, [sp, 64]
 ldp x25, x26, [sp, 48]
        ldp x23, x24, [sp, 32]
 ldp x21, x22, [sp, 16]
 ldp x19, x20, [sp], 128
        ret
