# Coverage map

Workers should APPEND to this file (don't rewrite) when they finish, listing what
they actually read (file + line range) and what they ruled out, so future workers
don't duplicate effort.

Format:
```
## worker-NN  YYYY-MM-DD
- File:lines — short note on what was examined
- Patterns ruled out: ...
- Potential bugs filed: candidates/NN-foo.md
```

## worker-07 2026-05-21
- File: llvm/lib/Target/X86/X86CmovConversion.cpp:1-917 — full read; focused on collectCmovCandidates, checkForProfitableCmovCandidates, convertCmovInstsToBranches, checkEFLAGSLive, packCmovGroup, memory-operand unfolding path.
- File: llvm/lib/Target/X86/X86DomainReassignment.cpp:1-862 — full read; focused on buildClosure/visitRegister/encloseInstr, initConverters (GPR->K mapping table), InstrReplacer::isLegal implicit-EFLAGS check, reassign/getDstRC.
- File: llvm/lib/Target/X86/X86FlagsCopyLowering.cpp:1-951 — full read; focused on hoist loop, HasEFLAGSClobber{,Path}, splitBlock, JmpIs rewriting loop, rewriteSetCC/rewriteArithmetic/rewriteMI, NF-variant fast path.
- Patterns ruled out:
  - Cmov SUBREG_TO_REG zero-extension check (line 340-348) appears defensively correct.
  - DomainReassignment InstrReplacer::isLegal correctly rejects converters that drop a live EFLAGS implicit-def (line 137-141).
  - FlagsCopy parity (P/NP) path through promoteCondToReg+SETCCr+TEST8rr+JNE is semantically equivalent.
- Potential bugs filed:
  - candidates/w07-domain-reassignment-shift-semantics.md — SHR/SHL imm overshift semantics differ from KSHIFT
  - candidates/w07-domain-reassignment-enclosed-edges-wrong-reg.md — `EnclosedEdges[Reg]` uses outer `Reg` instead of `CurReg` in buildClosure
  - candidates/w07-cmov-eflags-liveness-misses-jmp.md — checkEFLAGSLive misses EFLAGS readers spliced into SinkMBB
  - candidates/w07-flagscopy-hoist-clobber-window.md — hoist accepts HoistMBB without scanning its non-terminator body
  - candidates/w07-flagscopy-splitblock-iteration-after-split.md — splitBlock IsEdgeSplit leaves stale PHI entries when same successor across multiple JCCs
  - candidates/w07-cmov-load-unfold-eflags-clobber.md — chained CMOVrm unfolded loads inserted at static FalseInsertionPoint cause SSA reordering

## worker-08 2026-05-21
- File: llvm/lib/Target/X86/X86CompressEVEX.cpp:1-536 — full read; focused on tryCompressVPMOVPattern (cross-BB liveness for $kN), performCustomAdjustments (VRNDSCALE imm/SAE bits, VALIGN/VSHUF/VPERM2 imm rewrites), EVEX_B / EVEX_K bailout, NDD→LEA conversion.
- File: llvm/lib/Target/X86/X86InsertVZeroUpper.cpp:1-367 — full read; focused on isYmmOrZmmReg / clobbersAllYmmAndZmmRegs (YMM0-15/ZMM0-15-only), dirty-successor propagation, FirstUnguardedCall placement, livein detection.
- File: llvm/lib/Target/X86/X86AvoidStoreForwardingBlocks.cpp:1-749 — full read; focused on findPotentiallylBlockedCopies filters (no volatile/atomic check), breakBlockedCopies tail buildCopies call (LMMOffset vs SMMOffset), findPotentialBlockers cross-BB walk, alias().
- File: llvm/lib/Target/X86/X86AvoidTrailingCall.cpp:1-154 — full read; pass is Win64-only + WinCFI-only and correctly handles tail-call vs call distinction (isCallInstruction excludes returns) and DebugLoc. No issues spotted.
- File: llvm/lib/Target/X86/X86PadShortFunction.cpp:1-222 — full read; recursion guard via VisitedBBs, asserts ReturnLoc is RET not CALL, NOOP latency-based padding. No issues spotted; addPadding inserts plain NOOPs that don't affect unwind/cfi metadata.
- Patterns ruled out:
  - CompressEVEX VPMOV*2M → VMOVMSK semantic equivalence of sign-bit extraction (Q→PD, D→PS, B→PMOVMSKB) — all extract MSB of correct lane width.
  - CompressEVEX SrcVecReg-clobber-between-MI-and-KMOV is properly checked by `!KMovMI && CurMI.modifiesRegister(SrcVecReg)`.
  - CompressEVEX EVEX_B / EVEX_K / EVEX_L2 bailouts (lines 326-331, 374) correctly reject masked / 512-bit / SAE-rounding forms before they can be VEX-compressed.
  - InsertVZeroUpper iret special-case in X86_INTR functions (line 209-210) — intentional and correct.
  - AvoidTrailingCall: correctly inserts INT3 only when WinCFI present (line 92) and handles funclet boundaries.
  - PadShortFunction: cyclesUntilReturn excludes calls from the "return" count (line 197) so calls inside short functions don't trigger spurious padding.
- Potential bugs filed:
  - candidates/w08-sfb-volatile-atomic-not-checked.md — SFB silently splits a volatile/atomic 16-byte XMM/YMM memcpy into smaller copies (correctness)
  - candidates/w08-sfb-blocker-not-checked-for-volatile.md — SFB triggers on volatile blocker stores (related)
  - candidates/w08-sfb-buildcopies-wrong-mmo-offset.md — final buildCopies in breakBlockedCopies passes LMMOffset where SMMOffset is expected (latent, equal today)
  - candidates/w08-vzeroupper-clobbersAll-misses-y16-31.md — regmask "clean call" check only inspects YMM0-15/ZMM0-15
  - candidates/w08-compress-evex-vpmov-cross-bb-mask-use.md — MRI->use_operands check is a no-op for physregs post-RA; cross-BB mask live-out can yield uninitialized $kN
  - candidates/w08-compress-evex-vpmov-srcvec-clobber-after-kmov.md — kill-flag staleness when rewriting KMOV in place

## worker-10 2026-05-21
- File: llvm/lib/Target/X86/X86ExpandPseudo.cpp:1-980 — full read; TCRETURN family (incl. TCRETURN_WIN64ri / TCRETURNri64_ImpCall / TCRETURN_HIPE32ri), CALL_RVMARKER expansion, RET/IRET StackAdj math, LCMPXCHG16B_SAVE_RBX, MASKPAIR16{LOAD,STORE} (sub_mask_0/sub_mask_1 split, Disp+2), MWAITX_SAVE_RBX, ICALL_BRANCH_FUNNEL recursion + cmp/jcc + tailcall lowering, VASTART_SAVE_XMM_REGS block-splitting + AL test for SysV, EGPR/EVEX TILE pseudo set-desc paths, ADD/SUB/AND/OR/XOR/ADC/SBB *mi_ND length-limit split into MOV+RI.
- File: llvm/lib/Target/X86/X86CallingConv.cpp:1-409 — full read; CC_X86_32_RegCall_Assign2Regs (i64 mask split for regcall i386), CC_X86_VectorCall (32/64-bit HVA two-pass + shadow GPR allocation w/ XMM4/XMM5 shadow stack of 8), CC_X86_32_MCUInReg (split-arg <=2 GPR rule), CC_X86_Intr (single vs. error+frame layouts, 64-bit FIXME offset+=SlotSize), CC_X86_64_I128 (consecutive RDI/RSI/RDX/RCX/R8/R9 reg-block or 16-byte stack), CC_X86_32_I128_FP128 (always stack, 16-byte aligned), CC_X86_AnyReg_Error, CC_X86_64_Pointer (LP64 zext promotion).
- File: llvm/lib/Target/X86/X86CallingConv.h:1-33 — trivial proto wrapper, nothing actionable.
- File: llvm/lib/Target/X86/X86DynAllocaExpander.cpp:1-323 — full read; getDynAllocaAmount const-MOV recogniser, getLowering thresholds (Sub/TouchAndSub/Probe), computeLowerings RPO walk over CFG with ADJCALLSTACK{UP,DOWN} adjustments and SP-modifies fallback, lower() Sub/TouchAndSub fall-through using PUSH RAX/EAX, Probe path using emitStackProbe vs. no-stack-arg-probe SUB rr, AmountReg def cleanup.
- Patterns ruled out:
  - TCRETURN StackAdj/MaxTCDelta arithmetic uses MaxTCDelta<=0 invariant correctly; emitSPUpdate(InEpilogue=true) gets the post-mergeSPAdd offset.
  - LCMPXCHG16B_SAVE_RBX: Base==R(E)BX redirection to SaveRbx (and sub_32bit handling for EBX) is correct.
  - MASKPAIR16 split: Disp+2 with separate 2-byte MMOLo/MMOHi reflects two KMOVWk{m,mk}s of 16-bit halves of a 32-bit spill slot.
  - VASTART_SAVE_XMM_REGS: Win64 path correctly suppresses the AL==0 short-circuit because Win64 doesn't use AL to count XMM args.
  - DynAlloca on x32: StackPtr=ESP with getSubOpcode(Is64BitAlloca=false)=SUB32ri is consistent (the Is64Bit vs Is64BitAlloca distinction handles x32 correctly).
  - CC_X86_64_I128 stack fallback releases zero registers (consistent with C i128 ABI when not enough RDI..R9 remain in a contiguous block).
- Potential bugs filed:
  - candidates/w10-dynalloca-amount-zero-leaks-mov.md — Amount==0 fast path skips the AmountReg-def cleanup that the normal path performs.
  - candidates/w10-dynalloca-pushpop-misses-r8-r15.md — isPushPop() omits APX PUSH2/POP2, PUSHF/POPF, PUSH16, LEAVE — DynAlloca following such an SP-touching instr may be classed as not-touching and lowered to non-probing Sub.
  - candidates/w10-rvmarker-wrong-regmask-on-windows.md — CALL_RVMARKER always uses CallingConv::C preserved mask even when expanding on Windows; the surrounding code branches on isOSWindows() for the marker register (RCX vs RDI) but not for the regmask.

## worker-12 2026-05-21
- File: llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:18800-20554 — focused review of FP combines (visitFADD/FSUB/FMUL/FMA/FDIV/FNEG/FMinMax/FCOPYSIGN/FP_ROUND/FP_EXTEND/FREEZE/BUILD_VECTOR/EXTRACT_VECTOR_ELT) and 17964-18124 (visitFREEZE), 25191+ (visitEXTRACT_VECTOR_ELT).
- Patterns ruled out:
  - visitFP_ROUND double-round fold (20239-20269): correctly gates with `N0IsTrunc || (both AllowContract)`, combined IsTrunc is `NIsTrunc && N0IsTrunc`.
  - visitFADD `fmul B,-2.0 + A -> A - 2B` (18854-18865): worst-case differences are NaN-payload sign, not value.
  - eliminateFPCastPair (20293-20326): correctly requires nnan+ninf+contract on both casts.
  - visitFMA `(-N0*-N1)+N2 -> N0*N1+N2` (19368-19382): uses NegatibleCost::Cheaper gate.
  - visitFNEG `-(X-Y) -> Y-X` (20462-20466): correctly checks nsz.
  - visitFCOPYSIGN sign-mask -> disjoint-or (19848-19863): only fires when N0's sign bit is provably zero, so safe.
  - visitFREEZE BUILD_VECTOR all-undef path (18022-18034): explicitly preserves all-ones/constant classification with `getConstant(0)` for undef lanes — *not* the "all-undefs->zero" bug pattern.
  - visitBUILD_VECTOR all-undef -> UNDEF (26448-26449): returns UNDEF (not zero), correct.
- Potential bugs filed:
  - candidates/w12-fminimumnum-snan-not-quieted.md — visitFMinMax `MINIMUMNUM(X, NaN_const) -> X` returns X unchanged for any NaN constant; if X is sNaN at runtime, IEEE 754-2019 requires a qNaN result. **Highest-confidence**: directly reproducible on x86 with `llvm.minimumnum.f64`.
  - candidates/w12-fsub-negzero-fneg-snan.md — visitFSUB `fsub -0.0, X -> fneg X` (inline FIXME confirms); lowers to xorps sign-mask, doesn't quiet sNaN.
  - candidates/w12-fmul-neg1-fsub-snan.md — visitFMUL `fmul X, -1.0 -> fsub -0.0, X` pipes into the FSUB FIXME above; user's `fmul` ends up as raw FNEG with no NaN quieting.

## worker-03 2026-05-21
- Area: llvm/lib/Target/X86/X86ISelLowering.cpp lines ~26681..34532 (frame intrinsics, va_*, EH/trampoline, GET/SET/RESET_FPENV, GET_ROUNDING, ATOMIC_FENCE / CMP_SWAP / ATOMIC_STORE / lowerAtomicArith / lowerIdempotentRMWIntoFencedLoad, CTPOP / CTLZ / CTTZ, BITREVERSE / BITREVERSE_XOP, PARITY, CLMUL, LowerHorizontalByteSum, MSCATTER/MGATHER/MLOAD/MSTORE, ADDRSPACECAST, CVTPS2PH, PREFETCH, LowerOperation dispatch).
- Patterns ruled out:
  - CTPOP i2/i3/i4/i8 LUTs (`0x4332322132212110`, `0b1110100110010100`, `0x08040201` mul-shift) reconstructed by hand; constants correct.
  - LowerCTLZ i8 (zext-to-i32 then BSR + CMOV with `NumBits+NumBits-1` predawn, XOR `NumBits-1`) gives correct ctlz(0)=8/32/64.
  - LowerCTTZ scalar BSF + CMOV-NumBits and BitScanPassThrough paths return correct cttz(0)=NumBits; vector GFNI lowering uses `B = N0 & -N0` (LSB isolation) then GF2P8AFFINEQB with control 0x8.
  - LowerBITREVERSE PSHUFB nibble LUTs verified entry-by-entry; SRL-by-4 on vXi8 is safe (no need to mask high nibble).
  - INIT_TRAMPOLINE 64-bit (REX.WB 0x49 + movabsq r11/r10 + jmpq *r11) and 32-bit (MOV32ri+nest, JMP rel32 with `Disp = FPtr - (Trmp+10)`) byte sequences correct.
  - LowerVASTART 4-store SysV layout (gp_offset@0, fp_offset@4, overflow_arg@8, reg_save@16 LP64 / 12 ILP32) with parallel chains + TokenFactor is well-formed.
  - LowerGET_ROUNDING LUT `0x2d` (0b00_10_11_01) maps FPSR[11:10] → {1,3,2,0} correctly.
  - LowerATOMIC_FENCE only emits MFENCE/locked-stack-op for `seq_cst + SyncScope::System`, MEMBARRIER otherwise — correct on x86.
  - LowerCMP_SWAP chain wiring (CopyToReg + LCMPXCHG_DAG memintrinsic + CopyFromReg EAX/RAX + CopyFromReg EFLAGS) correct.
  - lowerAtomicArith OR-0 idempotent transform correctly bails on `AN->isVolatile()`.
  - LowerATOMIC_STORE i64 X87 FILD/FIST path round-trips i64 bit pattern through f80 significand losslessly.
  - LowerPARITY i8/i16/i32/i64 reduction-to-XOR-then-COND_NP correct.
- Potential bugs filed:
  - candidates/w03-resetfpenv-mmo-flags.md — LowerRESET_FPENV builds a MachineMemOperand with `MOStore` flag but attaches it to FLDENVm / ldmxcsr (which are LOADS from the constant pool). LowerGET_FPENV_MEM correctly re-flags to MOLoad before FLDENVm; LowerRESET_FPENV does not. Latent AA / scheduler bug.
  - candidates/w03-idempotent-rmw-drops-volatile.md — `lowerIdempotentRMWIntoFencedLoad` (and `isIdempotentRMW` in AtomicExpandPass) ignore the `volatile` flag, so `atomicrmw volatile or %p, i32 0 seq_cst` is rewritten to a non-volatile load. Verified with `llc -mtriple=x86_64-linux-gnu`: emits `lock orl $0, -64(%rsp); movl (%rdi), %eax` — identical to the non-volatile case, dropping volatility. Correctness bug for MMIO-style code.

## worker-06 2026-05-21
- File: llvm/lib/Target/X86/X86FixupBWInsts.cpp:1-504 — full read; `getSuperRegDestIfDead` super-reg liveness (incl. MOV-only IsDefined refinement at 252-282), `tryReplaceLoad`/`tryReplaceCopy`/`tryReplaceExtend` plumbing, MOVSX16rr8 AX/AL CBW carve-out (362-365), implicit-operand drop in tryReplaceCopy (346-348), processBasicBlock reverse iteration + delayed deletion (434-473).
- File: llvm/lib/Target/X86/X86FixupLEAs.cpp:1-950 — full read; postRAConvertToLEA opcode whitelist (172-220), optTwoAddrLEA Case1/Case2/Case3 LEA64_32r sub_32bit handling + implicit GR64 use (552-650), optLEAALU EFLAGS-dead guard + KilledBase/KilledIndex swap (488-550), checkRegUsage between-LEA-and-ALU scan, processInstrForSlow3OpLEA broadcast-when-base==index path (792-806).
- File: llvm/lib/Target/X86/X86FixupSetCC.cpp:1-164 — full read; FlagsDefMI hoist of XOR (`MOV32r0`), ABCD constraint for 32-bit (105-106), ZU branch (120-127), debug-instr-number redirect.
- File: llvm/lib/Target/X86/X86FixupInstTuning.cpp:1-710 — full read; NewOpcPreferable tput/lat/size ranking, ProcessVPERMILP{D,S}ri→VSHUFP{D,S}rri, ProcessVPERMILPSmi→VPSHUFDmi domain check, ProcessUNPCK{L,H}P{D,S} → integer/shufpd lowering, ProcessBLENDToMOV bit masks, ProcessVPERMQToVINSERT128 (0x44 -> vinserti128/vinsertf128) sub_xmm extraction, ProcessShiftLeftToAdd (PSLL*ri imm=1 → PADD*rr).
- File: llvm/lib/Target/X86/X86FixupVectorConstants.cpp:1-817 — full read; extractConstantBits (incl. UndefValue→0 at line 95), getSplatableConstant undef-tolerant ConstantVector path (176-209), rebuildConstant per-scalar dispatch, rebuildExtCst sext/zext fit-check, rebuildZeroUpperCst leading-zeros guard, FixupConstant ordering tables (FP loads, integer loads, EVEX broadcast fold via lookupBroadcastFoldTableBySize), AVX512→EVEX-bitop broadcast re-lowering (724-779).
- Patterns ruled out:
  - X86FixupBWInsts MOV8rr/MOV16rr with `sub_8bit_hi` (AH/BH/CH/DH) dest is correctly rejected by `getSuperRegDestIfDead` early-return at line 202-203, and source-side AH→AL "movb %ah,%al" is rejected by the sub-reg-index equality check at 331-333.
  - X86FixupBWInsts MOVSX16rr8 AX/AL carve-out (362-365) preserves CBW formation.
  - FixupSetCC EFLAGS-clobber MOV32r0 inserted *before* FlagsDefMI does not affect any later EFLAGS reader because FlagsDefMI itself redefines EFLAGS; the readsRegister guard at 100-102 handles the rmw-flags case.
  - FixupLEAs `postRAConvertToLEA` opcode whitelist (172-220) is strict.
  - FixupLEAs `optTwoAddrLEA` correctly serializes EFLAGS via `LQR_Dead` precondition at line 564.
  - FixupInstTuning `ProcessVPERMQToVINSERT128` (0x44 -> vinserti128 src1=src, src2=src.xmm, imm=1) is semantically correct: VPERMQ 0x44 broadcasts lower-128 to both lanes, VINSERTI128 with src1=src and src2=src.xmm reproduces dst.low=src.low and dst.high=src.low.
  - FixupVectorConstants rebuildExtCst sext fit-check `getSignificantBits() > SrcEltBitWidth` correctly rejects 0x80 as not 8-bit-sext-able.
  - FixupVectorConstants getSplatableConstant undef-tolerant path correctly stitches `<7,undef,7,undef>` → splat 7.
- Potential bugs filed:
  - candidates/w06-fixupsetcc-zu-assert.md — X86FixupSetCC asserts `SETZUCCr` on a SETCCr surviving GISel (X86InstructionSelector.cpp:1197,1300 emit SETCCr unconditionally without a ZU/preferLegacySetCC gate). Concrete repro: `-global-isel` + `-mattr=+zu` + a `(zext (setcc))` pattern.
  - candidates/w06-fixupinsttuning-pslli-loses-changed.md — `ProcessShiftLeftToAdd` mutates MI (PSLL*ri imm=1 → PADD*rr) but returns `false`, so the pass falsely reports "no changes". Breaks `NumInstChanges`, `Changed`, and `PreservedAnalyses` invalidation — stale machine analyses persist after a real mutation.
  - candidates/w06-fixupvectorconstants-rebuildext-undef-to-zero-bitop.md — `extractConstantBits` collapses top-level UndefValue to all-zeros while `getSplatableConstant` is undef-tolerant; two helpers disagree on the meaning of undef and produce table-order-dependent rewrites that are non-idempotent w.r.t. undef refinement and may diverge between otherwise-identical CP entries that differ only by which lanes were marked undef.

## worker-11  2026-05-21
- File: llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:1932-12500 (integer/bitwise visitor area). Spot-read visitADD/SUB/MUL/MULHS/MULHU/UMUL_LOHI/SMUL_LOHI, visitAND/OR/XOR, visitSHL/SRL/SRA, visitRotate/FunnelShift, visitSIGN_EXTEND_INREG/TRUNCATE/BITCAST, visitBSWAP/CTLZ/CTTZ/CTPOP, visitADDC/ADDE/UADDO/SADDO families, combineShiftOfShiftedLogic/visitShiftByConstant/foldAndToUsubsat/foldLogicOfShifts.
- Patterns ruled out:
  - visitSRA `(sra (shl X, m), n)` truncate fold (11322-11355) is guarded by simplifyShift so n in [1, BW-1]; no underflow.
  - visitSRL `(srl (shl x, c1), c2)` mask-fold (11568-11605): predicate ensures `Diff = |c2-c1| >= 0`, mask construction is correct.
  - visitFunnelShift `ShAmt==0 -> N0/N1` (11862-11864) is correct for non-pow-2 bitwidth per LangRef mod-BW semantics.
  - visitBSWAP single-byte known-bits fold (12296-12311): SHL `nuw` / SRL `exact` flag setting justified by leading/trailing-zero invariants.
  - visitMUL `(mul (shl X, c1), c2) -> (mul X, c2<<c1)` (4971-4977): correct mod 2^BW; flag-drop conservative.
  - visitADD `add (srl (not X), BW-1), C -> add (sra X, BW-1), C+1` (foldAddSubOfSignBit, 2851-2889): verified by 2-case truth table.
  - visitANDLike `(and (add C1), (srl (lshr y, c2)))` (7062-7090) — uses SRLC.getZExtValue() but predicate `SRLC.ult(VT.getSizeInBits()<=64)` so safe.
  - visitRotate `rot i16 X, 8 -> bswap X` (10733-10737): rotation amount is pre-normalized by mod-BW fold.
  - combineShiftOfShiftedLogic (10539-10607) — overflow + bitwidth checks present; LogicOp flag-copy investigated, no concrete miscompile.
- Potential bugs filed:
  - candidates/w11-mulhu-vec-one-splat.md — visitMULHU `mulhu x, (1<<c)` fold (5692-5704) has no vector splat-1 early-out (`isOneConstant` is scalar-only), so for vector splat 1 it constructs `SRL x, BW` which simplifyShift turns into UNDEF; result regresses guaranteed-zero into UNDEF. x86 currently masks via SimplifyDemandedBits, but the seed bug exists in generic combiner.
  - candidates/w11-sextinreg-extload-multiuse.md — visitSIGN_EXTEND_INREG (16843-16857) substitutes EXTLOAD with SEXTLOAD via CombineTo on all users when target says SEXTLOAD is legal, without `N0.hasOneUse()` guard; consumers expecting any-extend semantics may observe sign-extended high bits.
  - candidates/w11-shl-of-shifted-logic-disjoint.md — combineShiftOfShiftedLogic (10605) copies `LogicOp->getFlags()` (including `disjoint`) verbatim to the rewritten outer OR; flagged for further investigation.

## worker-01 2026-05-21
- X86ISelLowering.cpp:2890-2932 — mayFoldLoad / mayFoldLoadIntoBroadcastFromMem; noted that line 2930 only checks isVolatile() (not isAtomic()) before allowing broadcast-from-memory folding. Traced callers (lines 7990, 60055) — the actual broadcast-load conversion preserves the original LoadSDNode's MMO (so atomicity flags propagate) and the new VBROADCAST_LOAD still performs a single memory access of the same width. No semantic correctness bug filed; not strong enough.
- X86ISelLowering.cpp:4180-4920 — vector construct/extract helpers (getConstVector, getZeroVector, extractSubVector, insertSubVector, widenSubVector, widenMaskVector, insert1BitVector, concatSubVectors, collectConcatOps). Checked widenSubVector ZeroNewElements logic carefully (4391-4411) — correct for undef/zero upper-half optimization. insert1BitVector kshift index math (4756-4919) reviewed; the suspicious assert at 4791-4793 (`IdxVal % SubVecVT.getSizeInBits() == 0`) is only correct for 1-bit elements but that's the only caller — not a bug.
- X86ISelLowering.cpp:4965-5292 — vshift helpers, getEXTEND_VECTOR_INREG, getBitSelect, getPack. All look correct.
- X86ISelLowering.cpp:5766-5836 — IsNOT helper. PCMPGT C->C-1 transform correctly guards MinSignedValue at 5798-5810. Recursive cases construct fresh nodes, no in-place mutation of operands.
- X86ISelLowering.cpp:7188-9330 — BUILD_VECTOR lowering helpers: getBROADCAST_LOAD (isSimple+!isNonTemporal check 7196 — correct), LowerBuildVectorAsInsert/v16i8/v8i16/v4x32, LowerAsSplatVectorLoad (uses isSimple — 7599), findEltLoadSrc/EltsFromConsecutiveLoads (isSimple checks 7669, 7837 — correct), lowerBuildVectorAsBroadcast (8138-8369, ConstantPool/VBROADCAST paths reviewed; load path uses hasNUsesOfValue check), buildFromShuffleMostly, LowerBUILD_VECTORvXi1, ExpandHorizontalBinOp, isAddSubOrSubAdd/isFMAddSubOrFMSubAdd (FMA contract flag checked on the FMUL — correct), LowerToHorizontalOp, lowerBuildVectorToBitOp.
- X86ISelLowering.cpp:10216-10500 — LowerAVXCONCAT_VECTORS, LowerCONCAT_VECTORSvXi1 (zeros-MSB / KSHIFTL transform 10312-10326 verified), IsElementEquivalent.
- X86ISelLowering.cpp:11601-11916 — lowerShuffleAsBitMask, lowerShuffleAsBitBlend, matchShuffleAsBlend, lowerShuffleAsBlend (v16i16 split path 11815-11825 verified).
- X86ISelLowering.cpp:12770-13535 — matchShuffleAsShift, lowerShuffleAsShift, matchShuffleAsEXTRQ, lowerShuffleAsSpecificExtension (PSHUFLW/PSHUFHW OddEven selection at 13097 verified), lowerShuffleAsZeroOrAnyExtension, getScalarValueForVectorElement, lowerShuffleAsElementInsertion, lowerShuffleAsTruncBroadcast.
- X86ISelLowering.cpp:13591-14500 — lowerShuffleAsBroadcast (chain peek through bitcast/concat/extract/insert_subvector — bit-offset accounting verified, isSimple() at 13749 protects against vol/atomic), lowerShuffleAsInsertPS / matchShuffleAsInsertPS, lowerV2F64/V2I64/V4F32/V4I32Shuffle, lowerShuffleWithSHUFPS.
- Patterns ruled out: vol/atomic-load mishandling in the shuffle-broadcast/build-vector paths I read (all use isSimple()); FMF correctness in addsub/FMA; PCMPGT-C-1 underflow; undef-as-zero misuse in widenSubVector and LowerCONCAT_VECTORSvXi1; element-size mismatches in lowerBuildVectorToBitOp.
- Potential bugs filed: none. After detailed review the most suspicious site (mayFoldLoadIntoBroadcastFromMem missing isAtomic check at line 2930) does not appear to lead to a miscompile — the load is converted to a single VBROADCAST_LOAD that reuses the LoadSDNode's MMO, preserving atomic semantics, and the splat is a register-level broadcast. Not worth a verification cycle.

## worker-14 2026-05-21
Files audited:
- llvm/lib/Target/X86/GISel/X86InstructionSelector.cpp (full, 2020 lines)
- llvm/lib/Target/X86/GISel/X86LegalizerInfo.cpp (lines 1-620)
- llvm/lib/Target/X86/GISel/{X86CallLowering.cpp, X86RegisterBankInfo.cpp} (skipped)
- X86PreLegalizerCombiner.cpp / X86PostLegalizerCombiner.cpp do not exist in this tree.

Candidates filed:
- w14-uadde-cmp-inverted-carry.md — **CONFIRMED WRONG CODE**.
  selectUAddSub emits `CMP8ri carryIn, 1` before ADC/SBB to materialize CF
  from a previous SETCC byte. `CMP r,1` sets CF iff r<1 iff r==0, which is
  the inverse of the intended carry. Tiny i64 add via -global-isel on i386
  returns 0 instead of 1 (DAGISel returns 1). Repro included.

Other suspicions noted but not filed (mostly fall-through / "cannot select",
which the prompt says are NOT bugs):
- G_INSERT_VECTOR_ELT and G_EXTRACT_VECTOR_ELT are not in X86LegalizerInfo
  rules at all -> fall through.
- selectMergeValues/selectExtract require both src and dst to be vector;
  scalar G_MERGE_VALUES/G_EXTRACT will return false.
- selectCopy GR16<->XMM special cases eraseFromParent without `return true`,
  but then fall through to the final `return true` so functionally OK; the
  XMM->GR16 path also generates a bare `COPY GR32, XMM` which is not a real
  cross-bank instruction (relies on later RA handling). Worth a closer look
  if fuzzing surfaces miscompare on i16 extract from <8 x i16>.
- selectFCmp passes `LhsBank->getID() == X86::PSRRegBankID` to pick UCOM_FpIr
  vs UCOMISS; if the legalizer placed s32 fcmp args on PSR while the result
  bank is GPR, the SETCC dest reg-class derivation looks OK.

## worker-02  2026-05-21
- File: llvm/lib/Target/X86/X86ISelLowering.cpp lines ~15000..30000. The range is mostly shuffle lowering and "Lower*" custom lowering helpers (no top-level combine* functions yet at these line numbers; those live further down). Spot-read the higher-value targets:
  - 19138-19250 LowerVSELECT, 19253-19300 LowerEXTRACT_VECTOR_ELT_SSE4, 19304-19560 ExtractBit/InsertBitToMaskVector
  - 19565-19774 LowerINSERT_VECTOR_ELT, 19776-19840 **LowerFLDEXP** (CONFIRMED BUG below)
  - 19842-19960 LowerSCALAR_TO_VECTOR / LowerINSERT_SUBVECTOR / LowerEXTRACT_SUBVECTOR
  - 22196-22320 LowerTRUNCATE / truncateVectorWithPACK / matchTruncateWithPACK
  - 22729-22792 LowerLRINT_LLRINT / LRINT_LLRINTHelper
  - 22953-23175 LowerFP_EXTEND / LowerFP_ROUND (BUG below)
  - 23264-23334 lowerAddSubToHorizontalOp, 23348-23499 LowerFROUND / LowerFABSorFNEG / LowerFCOPYSIGN / LowerFGETSIGN
  - 23608-23752 combineVectorSizedSetCCEquality, 23827-24142 LowerVectorAllEqual / MatchVectorAllEqualTest / EmitTest
  - 24388-24530 getSqrtEstimate / getRecipEstimate / BuildSDIVPow2
  - 24711-24798 incDecVectorConstant / LowerVSETCCWithSUBUS, 24800-25272 LowerVSETCC (full)
  - 25631-26018 LowerSELECTWithCmpZero / LowerSELECT (full)
  - 26020-26452 LowerSIGN_EXTEND_Mask / Lower{ANY,SIGN}_EXTEND / EXTEND_VECTOR_INREG / splitVectorStore / scalarizeVectorStore / LowerStore / LowerLoad
- Patterns ruled out:
  - lowerAddSubToHorizontalOp HSUB/FHSUB commutation: HADD swap only fires for HADD/FHADD; HSUB correctly bails on odd LExtIndex. Lane-extract math for 256/512-bit verified.
  - LowerVSETCC v2i64 no-SSE42 PCMPGT(X, -1) sign-test shuffle: correct because i64 sign is the high-32 sign.
  - LowerVSETCCWithSUBUS SETULT bails on a constant zero element via incDecVectorConstant's `isZero` check.
  - LowerSELECTWithCmpZero `SplatLSB` truncation/extension to the splat width preserves the LSB correctly.
  - LowerINSERT_VECTOR_ELT cross-128 chunk index math (line 19686, 19690): `IdxIn128 = IdxVal & (NumEltsIn128-1)`, extract128BitVector rounds down to chunk boundary. Verified consistent.
  - combineVectorSizedSetCCEquality NeedZExt / NeedsAVX512FCast paths: VT computations for v16i8/v32i8/v64i8/v16i32 vectors check out.
  - LowerFCOPYSIGN sign-truncation/extension preserves the sign bit (FP_ROUND/FP_EXTEND on a value used only for sign).
  - splitVectorStore second-half align (passes BaseAlign rather than commonAlignment(BaseAlign, HalfOffset)) overstates alignment but only affects AA / scheduling, not correctness — long-standing pattern.
- Potential bugs filed:
  - candidates/w02-fldexp-missing-sint-to-fp-widened.md — **CONFIRMED WRONG CODE**. LowerFLDEXP widen-to-512 fallback (line 19832-19836) builds `SINT_TO_FP` on the non-widened `Exp` (element-count mismatch, dead value) and then feeds `WideExp` (raw integer subvector) directly to `X86ISD::SCALEF`, which expects FP. Generated `vscalefps %zmm1, %zmm0, %zmm0` is missing the required `vcvtdq2ps %xmm1, %xmm1`. The buggy CHECK lines in `llvm/test/CodeGen/X86/ldexp-avx512.ll` (AVX512F RUN line) document the bug as accepted output.
  - candidates/w02-strict-fp-extend-chain-drop.md — Strict `STRICT_FP_EXTEND f16 -> {f64,f80,fp128}` on non-FP16 targets (LowerFP_EXTEND line 22977-22981) builds outer `STRICT_FP_EXTEND` with chain input = the original `Chain`, dropping the inner extend's chain output. The two strict-fp ops become chain-siblings rooted at `Chain`, so other strict-fp ops in the same BB sharing `Chain` may legally reorder around the inner side effect.

## worker-04  2026-05-21
- File: llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp:1-3540 — full read with focus on simplify helpers (simplifyX86immShift, simplifyX86varShift, simplifyX86pack, simplifyX86pmulh, simplifyX86pmadd, simplifyX86movmsk, simplifyTernarylogic ~1000-line table spot-checked, simplifyX86FPMaxMin, simplifyX86insertps, simplifyX86extrq/insertq, simplifyX86pshufb, simplifyX86vpermilvar, simplifyX86vpermv, simplifyX86vpermv3, simplifyX86VPERMMask) and the main instCombineIntrinsic / simplifyDemandedVectorEltsIntrinsic switches.
- Patterns ruled out:
  - simplifyX86pshufb lane-masking (`(Index < 0 ? NumElts : Index & 0x0F) + (I & 0xF0)`) — correct for v16/v32/v64 i8.
  - simplifyX86vpermilvar PD bit-1 selector (`getLoBits(2)` + `lshrInPlace(1)`) and `SimplifyDemandedBits` mask `0b00010` for v8f64 — correct.
  - simplifyX86vpermv/vpermv3 index masking (`& (Size-1)` and `& (2*Size-1)`) — correct including v64i8/v32i16 cases.
  - simplifyX86pmulh PMULHRSW shift-then-trunc-to-i18 — equivalent to arithmetic shift + low-16 truncation.
  - simplifyX86pmadd PMADDUBSW i16 mul wrapping — products of zext(u8) × sext(s8) fit in i16 range, no truncation loss.
  - simplifyX86FPMaxMin NegZero asymmetric forbidding (Arg1 for maxnum, Arg0 for minnum) — verified against MAXPS pseudocode (both-zero → SRC2, NaN → SRC2).
  - simplifyX86_avx512_mask_*_ss/sd_round InsertElement(Arg0, V, 0) — correct because EVEX scalar destination upper 96 bits = SRC1 (= Arg0), not passthru.
  - simplifyTernarylogic switch table: spot-checked entries 0x3c, 0x5a, 0x66, 0x6c, 0x88, 0x96, 0xa0, 0xa5, 0xaa, 0xf0, 0xfa, 0xfc — all evaluate to their case label (the `assert(Res.second == Imm)` is the safety net).
  - simplifyX86insertq shuffle mask construction and shl-by-Index — Length=64+Index=0 corner case fine; APInt shift never reaches 64.
  - x86_avx512_mask_cmp_ss/sd shared body with vcomi only touches Arg0/Arg1 via SimplifyDemandedVectorEltsLow — predicate-imm not folded today.
  - sse_cvtss2si / cvttss2si family — only lane-0 demanded, no constant folding that would skip the invalid-input trap.
- Potential bugs filed:
  - candidates/w04-mask-cmp-ss-imm-immediate-not-validated.md — hazard sharing case body between mask_cmp_ss and comi/ucomi could grow into a predicate/SAE-losing fold.
  - candidates/w04-avx512-add-ps-512-cur-direction-MXCSR.md — `R == 4` (CUR_DIRECTION) folded to plain `fadd` ignores non-default MXCSR rounding inside fesetround regions.
  - candidates/w04-pmulh-multiply-by-one-undef-elements.md — `m_One()` matches `<i16 1, i16 undef, ...>` so unsigned PMULHUW collapses to zero vector, dropping data dependency on Arg1; signed PMULHW path is a borderline refinement.

## worker-05 2026-05-21
- File: llvm/lib/Target/X86/X86InstrInfo.cpp:111-1142 — isCoalescableExtInstr (incl. MOVSX64rr32 sub_32bit, 32-bit-mode bail for MOVSX*rr8), isDataInvariant/isDataInvariantLoad opcode lists, reMaterialize MOV32r->MOV32ri rewrite (SubIdx interaction), findRedundantFlagInstr AND+TEST16rr/TEST64rr+SUBREG_TO_REG pattern.
- File: llvm/lib/Target/X86/X86InstrInfo.cpp:3307-4159 — GetOppositeBranchCondition, getSwappedCondition, analyzeBranchImpl/analyzeBranchPredicate, removeBranch, insertBranch, canInsertSelect/insertSelect, copyPhysReg, CopyToFromAsymmetricReg, getLoadStoreOpcodeForFP16/getLoadStoreRegOpcode.
- File: llvm/lib/Target/X86/X86InstrInfo.cpp:4826-5757 — analyzeCompare, isRedundantFlagInstr (immDelta ±1, TEST*ri force-identical via CmpMask=0), optimizeCompareInstr (full review of MI/Sub/Movr0Inst/IsSwapped/ImmDelta/InstsToUpdate paths + APInt range guards), canConvert2Copy, convertALUrr2ALUri (HasNDDI ADD/SUB asymmetry vs. other ND opcodes), foldImmediateImpl / foldImmediate.
- File: llvm/lib/Target/X86/X86InstrInfo.cpp:5942-6500 — Expand2AddrUndef / Expand2AddrKreg / expandMOV32r1 / ExpandMOVImmSExti8 (push/pop+CFI, redzone bail) and expandPostRAPseudo (full opcode coverage: MOV32r*, SET0/AVX_SET0/AVX512_*_SET0, SETALLONES, SEXT_MASK, NOVLX loads/stores, MOV32ri64 expansion, RDFLAGS/WRFLAGS, MOVSHP).
- File: llvm/lib/Target/X86/X86InstrCompiler.td:315-365 — MOV32r0/r1/r_1/MOV32ImmSExti8/MOV64ImmSExti8 pseudo flags (Defs=[EFLAGS] / isReMaterializable / isPseudo) cross-checked against expansion behaviour.
- Patterns ruled out:
  - copyPhysReg AVX_SET0 / AVX512_*_SET0 super-reg widening (sub_xmm/sub_ymm → ZMM XOR) properly re-targets dest and adds ImplicitDefine of original.
  - reMaterialize MOV32r{0,1,_1} → MOV32ri rewrite when EFLAGS live: no EFLAGS implicit-def added (MOV32ri doesn't clobber EFLAGS) — correct.
  - optimizeCompareInstr's Movr0Inst hoist (5631-5651) is bounded to the same BB to avoid frequency regression and looks for an Instr that modifies-but-doesn't-read EFLAGS as the insertion point.
  - findRedundantFlagInstr's between-VregDef-and-CmpValDef EFLAGS-clobber scan (1108-1115) is sufficient because the caller's backward scan would have separately rejected EFLAGS-clobbers between CmpValDef and CmpInstr.
  - removeBranch / insertBranch / analyzeBranchImpl COND_NE_OR_P + COND_E_AND_NP synthesis (4128-4147, 3903-3933) cleanly round-trips through GetOppositeBranchCondition.
  - convertALUrr2ALUri ADC/SBB/AND/OR/XOR 64rr_ND → 64ri32_ND unconditional rewrite is correct because the rr_ND variants only exist on APX-NDD targets.
  - ExpandMOVImmSExti8 PUSH/POP redzone bail (6006-6011) is correct; falls back to MOV*ri when red zone is in use.
- Potential bugs filed:
  - candidates/w05-findRedundantFlagInstr-ND-AND-imm-operand-index.md — AND32ri_ND / AND64ri32_ND not in the operand-2 immediate filter at X86InstrInfo.cpp:1067-1070; **verified missed-opt** via `llc -run-pass=peephole-opt -mattr=+ndd`: swapping AND32ri ↔ AND32ri_ND turns a TEST16rr removal on/off.
  - candidates/w05-optimizeCmp-narrow-immDelta-signext.md — `APInt::getSignedMinValue(BitWidth) == CmpValue` (5560/5570/5580/5590) compares APInt as uint64_t while CmpValue is sign-extended int64_t; guard never fires for narrow-width SignedMin/Max edges. Plausible-but-not-yet-miscompile (the rewrite happens to be semantically valid at the boundary). Filed as fragile-guard / correctness risk.
  - candidates/w05-foldImmediate-COPY-ToReg-class-ignores-source.md — `foldImmediateImpl` keys the s32 range-check on the SOURCE register class but the NewOpc on the DESTINATION class; for cross-class COPY the chosen MOV32ri immediate operand can carry the wrong int64 representation and the GR64-dest path widens the def in ways that turn anyext upper-bits into a defined zero (subreg semantics drift).
  - candidates/w05-reMaterialize-MOV32r-loses-subreg-write.md — `reMaterialize` MOV32r0/1/_1 → MOV32ri rewrite then `substituteRegister(.., SubIdx, ..)` produces a 32-bit write tagged as a sub_8bit/sub_16bit subreg def; consumer's LiveIntervals may underestimate the bits actually clobbered.

## worker-17 2026-05-21
- File: llvm/lib/Target/X86/X86PartialReduction.cpp:1-563 — full read; focused on trySADReplacement split loop, tryMAddReplacement shrink check, matchAddReduction/collectLeaves, VPDPBUSD pattern.
- File: llvm/lib/Target/X86/X86InterleavedAccess.cpp:1-220 — read isSupported, decompose, reorderSubVector entry; did not find a clear miscompile in alignment or factor detection there (decompose uses commonAlignment correctly; VecBaseTy for 768/1536 case hardcodes a 16-byte vector type — alignment math holds).
- File: llvm/lib/Target/X86/X86PreTileConfig.cpp, X86OptimizeLEAs.cpp — exist but not deeply audited this slice.
- Patterns ruled out:
  - X86PartialReduction `CanShrinkOp(LHS) && CanShrinkOp(RHS)` looks like "&&" but transform is semantically NFC when only one side shrinks (SDAG just doesn't match pmaddwd later) — not a miscompile in this pass.
  - matchVPDPBUSDPattern signed/unsigned split appears correct (checks zext side via computeKnownBits<=8, sext side via ComputeMaxSignificantBits<=8).
- Potential bugs filed:
  - candidates/w17-psad-op1-uses-op0.md — trySADReplacement second-operand shuffle uses (Op1, Op0) instead of (Op1, Op1); for NumSplits>1 this silently substitutes Op0 chunks for Op1 chunks in the upper PSADBW calls. Definitive typo, clear miscompile candidate.

## worker-15 2026-05-21
- File: llvm/lib/Target/X86/X86FastISel.cpp:1-4071 — full read; focused on selectXxx methods (Load/Store/Ret/Cmp/ZExt/SExt/Branch/Shift/DivRem/Select/Trunc/FPExt/FPTrunc/IntToFP/BitCast), fastLowerCall, fastLowerIntrinsicCall, X86FastEmitCompare, getX86SSEConditionCode.
- Tested with `llc -O0 -fast-isel=true -mtriple=x86_64-unknown-linux-gnu`:
  - sext i1 -> i16, zext i1 -> i16, sext i8 -> i32: correct.
  - fcmp one/oeq/une/ueq float/double: correct (one→ucomiss+setne; ueq→sete; oeq/une→cmpeqss/cmpneqss; verified ZF/PF semantics).
  - trunc i32 -> i1 in conditional branch: emits `testl $1, %edi` then JCC — bit-0 semantics correct.
- Patterns ruled out:
  - X86SelectCmp FCMP_ONE/UEQ via getX86ConditionCode(COND_NE/COND_E) — semantically correct given UCOMISS flag layout: ZF=0 ⇔ ordered-and-distinct (ONE), ZF=1 ⇔ equal-or-unordered (UEQ).
  - X86SelectCmp FCMP_OEQ/UNE two-SETCC AND8rr/OR8rr pattern (COND_E+COND_NP / COND_NE+COND_P) correctly distinguishes ordered-equal from unordered-equal.
  - X86SelectCmp FCMP_FALSE path uses MOV32r0 (clobbers EFLAGS) before extracting sub_8bit; no pending EFLAGS users since FastISel emits in-order and FALSE is selected for the cmp itself.
  - X86SelectZExt SrcVT==i1 → fastEmitZExtFromI1 → i8 with 0/1, then i16 path uses MOVZX32rr8+extract sub_16bit, i64 path uses MOVZX32rr8+SUBREG_TO_REG sub_32bit; both bit-correct.
  - X86SelectSExt SrcVT==i1 → fastEmitZExtFromI1 then NEG8r (produces 0x00/0xFF in 8-bit RC), then MOVSX32rr8 for i16/i32/i64. Bit-correct.
  - X86SelectShift CL/CX/ECX/RCX dispatch + KILL of CL super-reg; x86 SH*8rCL/SH*16rCL/etc. implicitly mask shift amount, matching LLVM IR poison-on-overshift semantics (no extra mask needed at -O0).
  - X86SelectDivRem i8 SRem/URem AH-extraction via GR16 SHR16ri 8 (lines 2008-2024) correctly avoids GR8_NOREX issue on x86-64.
  - X86SelectBranch trunc-to-i1 fold uses TEST{8,16,32,64}ri imm=1 — checks bit-0 only, matches IR trunc-to-i1 semantics.
  - X86SelectBranch fallback path masks via TEST8ri OpReg, 1 — high garbage bits in i1-carrying i8 reg cannot leak.
  - X86SelectIntToFP IMPLICIT_DEF for FP source-passthrough operand — required for VCVTSI2SS*/VCVTUSI2SS* 3-operand AVX form.
  - X86SelectBitCast restricted to vector↔vector (skips i1-element vectors); bitcasts of equal size between v4f32/v4i32/v2f64 all map to VR128 with semantically-correct COPY.
  - fastLowerCall constant-arg pre-promotion (lines 3302-3344): when ConstantInt<32-bit is widened to i32, VT/OutVTs are recomputed from the (now-promoted) Val so CC analysis sees i32 — consistent.
  - fastLowerCall ByVal stores use TryEmitSmallMemcpy with ArgReg as src base — correct for small (<=32 byte) byval structs.
  - fastLowerIntrinsicCall x86_sse_cvttss2si InsertElement fold loop: walking past non-zero-index inserts is safe since CVTTSS2SI uses element 0; breaking at index 0 takes the inserted scalar.
  - fastLowerIntrinsicCall with-overflow umul/smul path: for i8 SMUL/UMUL copies LHS to AL/AX/etc. before IMUL8r/MUL*r single-operand forms.
- Potential bugs filed: none.
  Spent: ~10 areas spot-checked + 4 IR repros run through llc. The most suspicious sites
  (X86SelectCmp ONE→COND_NE, branch-on-trunc bit-0 test, ZExt/SExt from i1 negate
  path) all hold up on inspection and against real assembly output. The DivRem AH→GR16
  workaround on 64-bit is intentional. No clear miscompile or dead-bits violation
  identified in this slice.

## worker-18  2026-05-21
- File: llvm/lib/Target/X86/X86SpeculativeLoadHardening.cpp:1-2291 — full read; focused on hardenLoadAddr (vector/GR64 paths and EFLAGS save gate), tracePredStateThroughCFG cmov insertion, unfoldCallAndJumpLoads, tracePredStateThroughIndirectBranches (JMP64r-only).
- File: llvm/lib/Target/X86/X86SpeculativeExecutionSideEffectSuppression.cpp:1-198 — full read; OneLFENCEPerBasicBlock break vs continue, hasConstantAddressingMode NoRegister-vs-RIP comparison, OmitBranchLFENCEs path.
- File: llvm/lib/Target/X86/X86LoadValueInjectionLoadHardening.cpp:1-845 — full read; gadget graph build (RDF def-use), trimMitigatedEdges, hardenLoadsWithHeuristic ingress/egress cost, insertFences branch case (egress-CFG-edge cut after insertion).
- File: llvm/lib/Target/X86/X86LoadValueInjectionRetHardening.cpp:1-133 — full read; only X86::RET64 handled; findDeadCallerSavedReg fallback path.
- Patterns ruled out:
  - SLH `tracePredStateThroughCFG` uses inverted condition for split-edge cmov and non-inverted Cond on fallthrough — semantically correct (fallthrough means branch was speculated-not-taken, so any of the original taken conditions should poison).
  - SLH `unfoldCallAndJumpLoads` handles the full CALL{16,32,64}m / JMP{16,32,64}m{,_NT} / TAILJMPm{64,_REX} / TCRETURNmi{,64,_WIN} set before `hardenIndirectCallOrJumpInstr` is reached; FARCALL/FARJMP intentionally skipped per documented Spectre-non-applicability.
  - SESES `hasConstantAddressingMode` correctly classifies all JCCs as non-constant (via implicit EFLAGS), matching the documented design.
  - LVI Load Hardening conditional-branch transmitter check (instrUsesRegToBranch) only matches `isConditionalBranch` — indirect branches go through SOURCE/SINK via the memory-addressing check, not the branch check, so no gap there.
- Potential bugs filed:
  - candidates/w18-lvi-ret-missing-reti64-lret64.md — LVI ret-hardening only checks X86::RET64, misses RETI64/LRET64/IRET64.
  - candidates/w18-seses-onelfenceperbb-skips-branch.md — `-x86-seses-one-lfence-per-bb` uses `break` after load-LFENCE, dropping branch-LFENCE for the whole block.
  - candidates/w18-slh-shrx-eflags-no-bmi2-vector-skip.md — BMI2 gate at line 1654 skips saveEFLAGS even when vector+GR64 operands are mixed for AVX2 gather hardening.

## worker-13  2026-05-21
- File: llvm/lib/Target/X86/X86ISelDAGToDAG.cpp:1-6893 — full read; focused on address-mode matcher (matchAddress/matchAddressRecursively/matchAddressBase/matchVectorAddressRecursively/matchIndexRecursively/foldOffsetIntoAddress/matchLoadInAddress/matchWrapper), selectVectorAddr/selectAddr/selectLEAAddr/selectLEA64_Addr, tryFoldLoad/tryFoldBroadcast, matchVPTERNLOG/tryVPTERNLOG/tryMatchBitSelect/tryVPTESTM, foldLoadStoreIntoMemOperand (INC/DEC vs ADD imm, negate-shrink), tryShrinkShlLogicImm, emitPCMPISTR/emitPCMPESTR, Preprocess/PostprocessISelDAG, getAddressOperands (segment/displacement encoding).
- Patterns ruled out:
  - `foldOffsetIntoAddress` 1850 — `int64_t Val = AM.Disp + Offset` is safe under unsigned-wrap arithmetic; symbolic ES/MCSym non-zero displacement is rejected.
  - `selectMOV64Imm32` Kernel/Large early-out is correct.
  - `getAddressOperands` correctly threads ES/MCSym/JT assertions and emits signed-target-constant for plain Disp.
  - PostprocessISelDAG ANDrm→TEST*mr peephole IS guarded by `hasNUsesOfValue(2, ResNo)` on the AND value-0 (line 1638) so multi-use AND-GR is rejected.
  - `foldLoadStoreIntoMemOperand` imm-form selection is safe because `isFusableLoadOpStorePattern` constrains StoredVal type to StoreNode->getMemoryVT (so OperandV magnitude can't exceed MemVT).
  - INC/DEC vs ADD-1 EFLAGS: `hasNoCarryFlagUses` is invoked and OF/SF/ZF semantics of `ADD x,K` ↔ `SUB x,-K` are mathematically equivalent (verified by case analysis of the OF predicate).
  - `tryShrinkShlLogicImm` AND-MOVZX bailout (line 4674) correctly checks MaskedValueIsZero before allowing imm reorder.
  - `isEndbrImm` sign-extension comparison (line 978-980) is consistent because both sides are int64.
- Potential bugs filed:
  - candidates/w13-matchvectoraddress-no-wrapperrip.md — `matchVectorAddressRecursively` only handles `X86ISD::Wrapper`, not `WrapperRIP`; gather/scatter base over a RIP-relative global falls through to matchAddressBase, missing the disp32 fold and potentially mishandling TLS Wrapper flags.
  - candidates/w13-foldoffset-mul-imm-uint64-overflow.md — MUL-by-{3,5,9} LEA shortcut uses `(int64) * (uint64)` mixed-sign multiplication for the constant addend in `(X+c)*N`; corner cases with negative `AddVal` and 32-bit wraparound semantics deserve fuzzer focus.
  - candidates/w13-emitPCMPESTR-fold-load-misses-eax-edx-glue.md — `emitPCMPESTR` fold-load path sequences EAX/EDX preloads via glue only; chain provenance of the implicit live-in CopyToReg is not preserved on CNode, leaving room for aliasing reordering after later passes drop glue edges.

## worker-16 2026-05-21
- File: llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp:1-3540 — re-audit of TTI overrides
  (focus: instCombineIntrinsic + simplifyDemandedVectorEltsIntrinsic +
  simplifyDemandedUseBitsIntrinsic). Largely overlaps with worker-04's slice.
- File: llvm/lib/Target/X86/X86TargetTransformInfo.{cpp,h} — verified that the
  cost-model file does not contain the three semantic overrides (they live in
  X86InstCombineIntrinsic.cpp).
- Patterns ruled out (cross-checked against worker-04's already-filed leads):
  - simplifyX86varShift undef-shift-amount → emits IR `shl x, undef` which is
    poison; the original intrinsic with undef shift-amount lane is undef per
    LLVM convention, so undef → poison is a permitted refinement. Not a bug.
  - simplifyX86pshufb `(Index < 0)` masking and (I & 0xF0) lane base for v16/v32/v64
    — out-of-bounds shuffle indices for the zero-vector side stay in
    [NumElts, 2*NumElts) for both 256-bit and 512-bit variants. Verified.
  - getNegativeIsTrueBoolVec with undef lanes in constant masks (used by blendv
    and maskmov simplification) produces `select i1 undef, A, B` which is
    refinement-compatible with the x86 maskmov "undef sign bit" semantics.
  - simplifyX86vpermv3 (VPERMI2) operand order: code uses V1=arg0, V2=arg2 with
    Index in [0,N) → V1, [N,2N) → V2. Hardware: idx high-bit selects arg2.
    Confirmed match.
  - simplifyX86vpermilvar PD variants `getLoBits(2) + lshrInPlace(1)` keeps only
    bit 1 — correct (matches spec).
  - simplifyDemandedVectorEltsIntrinsic PMADD case (`ScaleBitMask` to inner width)
    is correct for both PMADDWD and PMADDUBSW (out_i depends on input pair 2i, 2i+1).
  - simplifyDemandedUseBitsIntrinsic MOVMSK family: `DemandedMask.zextOrTrunc(ArgWidth)`
    correctly returns null when only the known-zero high bits of the i32 result
    are demanded.
  - simplifyTernarylogic single-operand imm cases (0xF, 0x33, 0x55, 0xAA, 0xCC, 0xF0, 0xFF)
    correctly degenerate to NOT/COPY of the relevant operand without depending on
    others — safe because the output doesn't depend on those operands at all.
  - simplifyX86immShift CDV-based 64-bit shift-amount reconstruction: only
    iterates the bottom-64-bits sub-elements (correctly ignores upper 64 of the
    XMM shift-amount), and the `dyn_cast<ConstantDataVector>` gate excludes
    undef-containing vectors so no cast crash.
- Potential bugs filed: none.
  Spent: ~6 candidate sites scrutinized in depth, all resolved to "by design"
  or refinement-permitted. Worker-04 had already filed the strongest leads in
  this file (w04-mask-cmp-ss-imm-immediate-not-validated.md,
  w04-avx512-add-ps-512-cur-direction-MXCSR.md,
  w04-pmulh-multiply-by-one-undef-elements.md). I did not find any additional
  correctness bugs not already covered there.

## worker-20 2026-05-21
- File: llvm/lib/Target/X86/X86WinEHState.cpp:1-868 — full read; focused on addStateStores RPOT loop, getPredState/getSuccState join-point logic, cleanup-pad skip at line 779, isStateStoreNeeded, rewriteSetJmpCall InCleanup path.
- File: llvm/lib/Target/X86/X86WinEHUnwindV2.cpp:1-449 — full read; epilog/prolog state machine matched expectations, POP-reverse-order check is tight, SetFrameBack handling is correct.
- File: llvm/lib/Target/X86/X86PreTileConfig.cpp:1-469 — full read; isDestructiveCall semantics match comments (any AMX clobber triggers), CfgLiveInBBs propagation looks OK, hoistShapesInBB has mayLoadOrStore guard.
- File: llvm/lib/Target/X86/X86TileConfig.cpp:1-234 — full read; no correctness bug retained.
- File: llvm/lib/Target/X86/X86FastTileConfig.cpp:1-213 — full read; per-BB ShapeInfos collection misses cross-BB tile defs whose value reaches a PLDTILECFGV in a successor.
- File: llvm/lib/Target/X86/X86LowerAMXType.cpp:1-50, 200-470, 540-740 — skimmed combine/PHI-volatile logic; nothing obviously wrong.
- Patterns ruled out:
  - WinEHUnwindV2 epilog start-location for stack-dealloc-without-pop case (UnwindV2StartLocation defaults to MI at SEH_EndEpilogue, looks intentional).
  - PreTileConfig's TileCfgForbidden propagation correctly avoids inserting LDTILECFG before shape defs.
  - X86TileConfig SS lookup uses the FIRST PLDTILECFGV but every PreTileConfig insertion uses the same stack slot, so this is safe.
- Potential bugs filed:
  - candidates/w20-fasttileconfig-cross-bb-shape.md — FastTileConfig misses tile shapes whose defs are in a different BB than the PLDTILECFGV
  - candidates/w20-winehstate-cleanup-skip-loses-hoist.md — cleanup-pad BB skip in addStateStores can leave successor blocks with stale PrevState (-1)

## worker-19 2026-05-21
- File: llvm/lib/Target/X86/X86IndirectBranchTracking.cpp:1-198 — full read; runIndirectBranchTracking, addENDBR, needsPrologueENDBR, EHPad/SjLj branches, returns_twice handling.
- File: llvm/lib/Target/X86/X86IndirectThunks.cpp:1-243 — full read; RetpolineThunkInserter & LVIThunkInserter populate, register-class choice for 32-bit thunks.
- File: llvm/lib/Target/X86/X86ReturnThunks.cpp:1-117 — full read; ret-opcode matching, CS_PREFIX path, TAILJMPd substitution.
- File: llvm/lib/Target/X86/X86MCInstLower.cpp:903-947 — LowerKCFI_CHECK; TempReg selection, ADD32rm offset -(PrefixNops+4), JCC_1 layout.
- Patterns ruled out:
  - KCFI TempReg choice (R10 vs R11): KCFI_CHECK ptr operand is GR64 (X86InstrCompiler.td:286), so sub-register confusion is impossible.
  - Retpoline 64-bit thunk register choice (R11) is correct and matches calling convention.
  - LVI thunk emission is symmetric and correct.
  - ENDBR after returns_twice call mid-block lands correctly in the same MBB (verified with llc).
  - IBT prologue ENDBR correctly emitted for externally-linked or address-taken functions.
- Potential bugs filed:
  - candidates/w19-returnthunks-missing-reti-lret-iret.md — X86ReturnThunks pattern-matches RET64/RET32 only; misses RETI*/LRET*/IRET* (parallel to w18 LVI bug; reproduced with i686 stdcall).
  - candidates/w19-ibt-wineh-funclet-no-endbr.md — WinEH catch/cleanup funclet entry MBBs reached indirectly by the OS dispatcher get NO endbr64; verified on x86_64-pc-windows-msvc.

## worker-21 2026-05-21
- File: llvm/lib/CodeGen/SelectionDAG/SelectionDAGBuilder.cpp — focused review of visitAtomicLoad/Store/RMW/CmpXchg (5196-5381), visitMasked{Load,Store,Gather,Scatter} (4944-5193, incl. getUniformBase), visitMemSet/MemCpy/MemMove + element_unordered_atomic (6695-6804), visitVAStart/Arg/Copy/End (10812-10848), frame intrinsics + returnaddress/sponentry/addressofreturnaddress (6649-6671), is_fpclass (7204-7229), get/set/reset_fpenv + get/set_fpmode (7231-7307), stacksave/stackrestore/get_dynamic_area_offset (7498-7520), ldexp/frexp/sincos/modf/sincospi multi-result (7054-7087), visitConstrainedFPIntrinsic + fmuladd break path (8545-8620), visitVectorReverse/Splice/Deinterleave/Interleave (12876-12969), visitVectorReduce (11189-11263), visitTargetIntrinsic (5514-5573), assume/expect/launder/strip (7580-7663), eh.sjlj (6856-6889).
- Patterns ruled out:
  - visitAtomicCmpXchg/RMW/Load/Store all thread chain correctly via getRoot()/setRoot(OutChain); volatile+atomic flags flow through TLI.get*MemOperandFlags into the MMO; range metadata on atomic load is permitted by LangRef.
  - getUniformBase splat-constant base path (5008-5019) correctly extracts a scalar splat value as Base and zero Index; non-splat constants bail.
  - visitMasked{Gather,Scatter} use `getMemoryRoot()` (acceptable: masked.gather/scatter have no volatile attribute per LangRef); the `shouldExtendGSIndex` API mutates EltTy by reference, so the apparent no-op SIGN_EXTEND at 5083-5086/5183-5186 only fires when the target widened EltTy.
  - visitVectorDeinterleave EXTRACT_SUBVECTOR-then-VECTOR_DEINTERLEAVE matches ISDOpcodes.h spec (deinterleave operates on conceptual CONCAT_VECTORS of N subvectors).
  - visitVectorSplice IsLeft/IsRight dispatch and Idx = (IsLeft ? Imm : NumElts - Imm) matches LangRef.
  - visitConstrainedFPIntrinsic fmuladd break (8578-8593): STRICT_FMUL chain is propagated to STRICT_FADD via Mul.getValue(1); pushing both chains is harmless (graph dedup) and intentional (both go into the same PendingConstrainedFP list).
  - element_unordered_atomic memcpy/memmove/memset all use `getRoot()` (not getMemoryRoot()) — strict serialization for atomic ops, correct.
  - is_fpclass NoFPExcept-vs-StrictFP-attribute inversion (7214) is strictly conservative (blocks opts, doesn't miscompile); not filed.
  - Intrinsic::assume discarded with no chain touch (7598-7604) is safe — operand is an already-visited SSA value, side effects of any contributing IR instr are independently lowered.
- Potential bugs filed: none. The strongest pre-existing candidate in this area (w02-fldexp-missing-sint-to-fp-widened.md — the bug for which this very bug-004 directory was created) and w03-resetfpenv-mmo-flags.md already cover the LowerFLDEXP and RESET_FPENV mistakes. Did not find any additional independent miscompile-class bugs in SelectionDAGBuilder.cpp not already covered by workers 02/03.

## worker-24 2026-05-21
- File: llvm/lib/CodeGen/AtomicExpandPass.cpp:1-2262 — focused read on idempotent-RMW path, partword RMW/cmpxchg helpers (createMaskInstrs, extract/insertMaskedValue, performMaskedAtomicOp, expandPartwordAtomicRMW, widenPartwordAtomicRMW, expandPartwordCmpXchg), cmpxchg-to-libcall (expandAtomicCASToLibcall, canUseSizedAtomicCall), FP RMW lowering case in performMaskedAtomicOp.
- File: llvm/lib/CodeGen/ExpandReductions.cpp:1-213 — full read; checked all reduction kinds, identity selection cross-referenced with LoopUtils.cpp:getReductionIdentity (1493-1533) and getShuffleReduction (1405-1461).
- File: llvm/lib/CodeGen/ExpandVectorPredication.cpp:1-741 — full read of CachingVPExpander, expandPredicationIn{BinaryOperator,Reduction,Comparison,MemoryIntrinsic,Cast}, foldEVLIntoMask/discardEVLParameter/convertEVLToMask, sanitizeStrategy.
- File: llvm/lib/CodeGen/ExpandIRInsts.cpp:1170-1215 — only checked references to the merged-in ExpandLargeFp/ExpandLargeDivRem entry points (no deep read).
- Cross-ref: llvm/lib/Target/X86/X86ISelLowering.cpp:33000-33058 (lowerIdempotentRMWIntoFencedLoad) — confirms the same volatile-stripping flaw already filed under w03.
- Patterns ruled out:
  - createMaskInstrs (line 963-967) "1 << (ValueSize * 8) - 1" is technically UB if ValueSize == 4, but no target currently sets MinCmpXchgSizeInBits > 32, so the UB shift never fires (ValueSize is at most 2 in practice). Not filed.
  - expandPartwordAtomicRMW correctly threads AI->isVolatile() through to insertRMWCmpXchgLoop.
  - ExpandReductions: vector_reduce_fmax/fmin path correctly requires noNaNs() and uses shuffle reduction; getReductionIdentity selects QNaN/-Inf based on PropagatesNaN/nnan correctly (matches non-NaN-propagating semantics of vector.reduce.fmax in current LangRef).
  - ExpandVectorPredication.expandPredicationInBinaryOperator drops mask on non-div ops; safe because masked-off lane semantics are poison and a concrete value is a valid refinement of poison.
  - ExpandVectorPredication.expandPredicationInReduction: for vp_reduce_{fmax,fmin,fmaximum,fminimum,smin,smax,umin,umax}, neutral identity insertion + scalar minmax intrinsic on Start is correct; for vp_reduce_fadd/fmul ordered reduction is correctly emitted via CreateF{Add,Mul}Reduce.
  - expandAtomicCASToLibcall passes correct alignment, success/failure ordering, value/expected operands to expandAtomicOpToLibcall.
- Potential bugs filed:
  - candidates/w24-widenpartword-atomicrmw-drops-volatile.md — widenPartwordAtomicRMW (line 1141-1143) constructs a fresh atomicrmw via IRBuilder::CreateAtomicRMW without ever calling setVolatile(AI->isVolatile()). On targets with MinCmpXchgSizeInBits > value width (RISC-V w/o Zabha, LoongArch base, Sparc, AMDGPU, Hexagon, VE, Xtensa), a `volatile atomicrmw {and,or,xor}` of an i8/i16 silently loses its volatile flag in the widened i32 RMW, allowing later DSE/LICM/GVN to elide or hoist what the user wrote as a single volatile MMIO access.
- Note: The QUEUE.md assignment for w24 was SelectionDAG.cpp + TargetLowering.cpp (generic helpers); however the dispatched task message routed me to the wave-3 expansion-pass cluster (originally w25). The SelectionDAG.cpp / TargetLowering.cpp generic-helper sweep remains unallocated.

## worker-22  2026-05-21
- LegalizeDAG.cpp:5025-5090 — found `FLDEXP`/`STRICT_FLDEXP` libcall expansion
  has **no** exponent-size guard (unlike adjacent FPOWI which errors and unlike
  SoftenFloatRes_ExpOp which guards). For `i64` exponent it silently emits a
  `ldexp@PLT` tail call passing `%rdi`, while `ldexp(double,int)` reads only
  `%edi`. Filed as `w22-fldexp-libcall-no-exponent-size-check.md`.
  Confirmed for `f32`, `f64`, `x86_fp80`, `fp128` on `-mtriple=x86_64`.
- LegalizeVectorTypes.cpp:533-547 (`ScalarizeVecRes_UnaryOpWithExtraInput`) —
  used only for FPOWI/AssertSext/AssertZext/AssertNoFPClass; safe.
- LegalizeVectorTypes.cpp:1886-1905 (`SplitVecRes_CONCAT_VECTORS`) — correct
  halving of operand list, no off-by-one.
- LegalizeVectorTypes.cpp:2000-2021 (`SplitVecRes_FPOp_MultiType`) — used for
  FPOWI/FLDEXP/FCOPYSIGN. RHS split or scalar broadcast both look correct.
- LegalizeVectorTypes.cpp:5604-5692 (`WidenVecRes_BinaryCanTrap`) and
  6157-6170 (`WidenVecRes_ExpOp`) — looked clean.
- LegalizeVectorTypes.cpp:6815-6830 (`WidenVecRes_VECTOR_COMPRESS`) — passthru
  widened with poison fill; output positions past EVL/popcount unused, so safe.
- LegalizeVectorTypes.cpp:6832-6889 (`WidenVecRes_MLOAD`) — VP_LOAD path uses
  poison-extended mask + correct EVL; masked path fills mask with zeros. OK.
- LegalizeVectorTypes.cpp:8126-8200 (`WidenVecOp_MSTORE`/`MGATHER`) — Index
  widening for MGATHER is allowed-larger; mask zero-fill in MSTORE is correct.
- LegalizeFloatTypes.cpp:731-773 (`SoftenFloatRes_ExpOp`) — has the
  size-of-int guard. Error message mentions "powi" even when called for ldexp;
  cosmetic only.
- LegalizeFloatTypes.cpp:1720-1733, 2062-2065 (`ExpandFloatRes_FLDEXP` via
  `ExpandFloatRes_Binary`) — no guard either, but only reachable for
  ppcf128 on PPC; on x86 the scalar libcall expansion in LegalizeDAG hits first
  (already covered by candidate above).
- LegalizeIntegerTypes.cpp:907-940 (`PromoteIntRes_FP_TO_XINT`) — adds proper
  AssertSext/AssertZext; OK.
- LegalizeIntegerTypes.cpp:4641-4651 — saturating add/sub/shl just defer to
  TLI.expandAddSubSat / expandShlSat. Not investigated.

## worker-23 2026-05-21
- File: llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp:6256-6453 — full read of isKnownNeverNaN switch (FADD/FMUL/.../FLDEXP, FMINNUM family, FP_EXTEND/ROUND, SINT_TO_FP/UINT_TO_FP, EXTRACT/INSERT_SUBVECTOR/VECTOR_ELT, BUILD_VECTOR, SPLAT_VECTOR, AssertNoFPClass).
- File: llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp:3358-3460,4426-4442,4903-5100 — computeKnownBits for SINT_TO_FP/UINT_TO_FP/FP_TO_UINT_SAT; ComputeNumSignBits for SRA/SHL/SIGN_EXTEND/SIGN_EXTEND_INREG/FP_TO_SINT_SAT/VECTOR_SHUFFLE/BUILD_VECTOR/BITCAST.
- File: llvm/lib/CodeGen/SelectionDAG/TargetLowering.cpp:658-2400 — SimplifyDemandedBits scaffolding + AND/OR/XOR/SHL/SRL/SRA/FSHL/FSHR/SETCC/AVG paths.
- File: llvm/lib/CodeGen/SelectionDAG/TargetLowering.cpp:3143-3920 — SimplifyDemandedVectorElts scaffolding + CONCAT_VECTORS/INSERT_SUBVECTOR/EXTRACT_SUBVECTOR/INSERT_VECTOR_ELT/VSELECT/VECTOR_SHUFFLE.
- File: llvm/lib/CodeGen/SelectionDAG/TargetLowering.cpp:4715-5570 — SimplifySetCC top + FNEG/fabs-Inf folds, X*Y==0 nuw/nsw decomposition, SIGN_EXTEND_INREG eq fold.
- File: llvm/lib/CodeGen/SelectionDAG/TargetLowering.cpp:6570-6603 — buildSDIVPow2WithCMov.
- File: llvm/lib/CodeGen/SelectionDAG/TargetLowering.cpp:7900-8090 — expandMUL_LOHI/expandMUL (signed/unsigned shortcut, Karatsuba-style with adjust).
- File: llvm/lib/CodeGen/SelectionDAG/TargetLowering.cpp:10637-10720 — expandABS / expandABD.
- File: llvm/lib/CodeGen/SelectionDAG/TargetLowering.cpp:12502-12577 — expandMULO (power-of-2 shortcut for both signed and INT_MIN case, wide-mul fallback).
- Patterns ruled out:
  - expandMUL_LOHI Karatsuba sign correction (lines 8059-8067) correctly subtracts RL/LL based on LH/RH sign.
  - expandMULO power-of-2 with C = SignedMin (INT_MIN) correctly uses SRL rather than SRA (since smulo(x,INT_MIN) == umulo(x,INT_MIN)).
  - buildSDIVPow2WithCMov correctly handles negative power-of-2 divisor via final negation.
  - SimplifyDemandedBits SRA Log2-fold (lines 2199-2204) is gated by the earlier `countl_zero >= ShAmt` check so Log2 is always in the sign-extended region when the fold fires.
  - SimplifyDemandedBits FSHR-to-SRL fold (lines 2280-2298) `countl_zero(DemandedBits) >= MaxShiftAmt` correctly proves Op0 contributes no demanded bit.
  - expandABS / ABS_MIN_POISON->ABS substitution is poison-refining (well-defined replacing poison), safe.
  - isKnownNeverNaN(SINT_TO_FP/UINT_TO_FP) returning true: correct (integer-to-FP never produces NaN even on overflow-to-infinity).
  - SimplifySetCC fneg-operand-swap fold preserves NaN semantics (ordered/unordered both still false/true).
  - SimplifySetCC fcInf is_fpclass folds for fabs/Inf correctly produce fcNone for impossible cases and OR in fcNan for unordered preds.
  - computeKnownBits UINT_TO_FP makeNonNegative is correct (uint-to-fp result is always +0 or positive).
- Potential bugs filed:
  - candidates/w23-isKnownNeverNaN-fminnum-snan-or-incorrect.md — FMINNUM/FMAXNUM/FMINIMUMNUM/FMAXIMUMNUM use OR-logic for SNaN=true, but these variants do NOT quiet NaN inputs per LangRef, so an SNaN operand can propagate through the both-NaN tie-breaker even when the other operand is known never-SNaN (but possibly QNaN).

## worker-30  2026-05-21
- File: llvm/lib/CodeGen/MachineSink.cpp:1-2405 — read PerformSinkAndFold, FindSuccToSinkTo, SinkInstruction, blockPrologueInterferes, hasStoreBetween, hasRegisterDependency, tryToSinkCopy (post-RA), SinkingPreventsImplicitNullCheck. EFLAGS guard at 1881-1890 only checks SuccToSinkTo->isLiveIn; no scan of source-block tail for a stale-dead physreg.
- File: llvm/lib/CodeGen/MachineLICM.cpp:1-1738 — read IsLICMCandidate, IsLoopInvariantInst, IsGuaranteedToExecute, HoistRegion, HoistRegionPostRA, isInvariantStore, HasLoopPHIUse, HasHighOperandLatency, IsCheapInstruction. IsGuaranteedToExecute only checks dom of exiting blocks; ignores intra-loop mayThrow.
- File: llvm/lib/CodeGen/MachineCSE.cpp:1-979 — read hasLivePhysRegDefUses, PhysRegDefsReach, isCSECandidate, ProcessBlockCSE (implicit-def/PhysDef positional updates at 631-689).
- File: llvm/lib/CodeGen/PeepholeOptimizer.cpp:1450-1548 — read foldImmediate, foldRedundantCopy. foldImmediate iterates explicit operands only; no tied-operand check before calling TII->foldImmediate.
- Patterns ruled out:
  - MachineSink hasStoreBetween (1657-1750) properly tracks alias and handles ordered-memref/calls conservatively.
  - MachineCSE isCSECandidate (398-430) correctly rejects mayStore/isCall/isTerminator/mayRaiseFPException/hasUnmodeledSideEffects/INLINEASM/LOAD_STACK_GUARD; load only allowed if isDereferenceableInvariantLoad.
  - MachineCSE hasLivePhysRegDefUses (282-330) correctly aliases physreg defs via MCRegAliasIterator and uses isPhysDefTriviallyDead before adding to PhysDefs.
  - MachineLICM IsLICMCandidate (1078-1110) checks isSafeToMove first; convergent check present; load-not-guaranteed-to-execute gate present.
  - MachineSink PerformSinkAndFold (405-465) correctly bails on isConvergent, multiple defs, more than two reg uses, and any physreg use/def except ignorable.
  - PostRA tryToSinkCopy (2242-2372) only sinks renamable copies; uses LiveRegUnits accumulation.
- Potential bugs filed:
  - candidates/w30-machinesink-physdef-dead-not-zombie-checked.md — sink EFLAGS guard at 1881-1890 trusts upstream `dead` flag and only checks successor live-in, missing tail-of-source-block readers of the same physreg.
  - candidates/w30-machinelicm-isguaranteedtoexecute-ignores-throwing-inline-asm.md — IsGuaranteedToExecute equates "dominates exiting blocks" with "executes every iteration"; ignores earlier mayThrow/INLINEASM-sideeffect.
  - candidates/w30-machinecse-implicit-def-positional-mismatch.md — ImplicitDefsToUpdate and PhysDefs positional indexing into CSMI assumes operand-layout parity with MI; only loops MI.getNumOperands; subtle robustness bug when implicit-def lists differ.
  - candidates/w30-peephole-foldimmediate-no-tied-operand-check.md — foldImmediate offers any explicit non-def operand to TII without checking isRegTiedToDefOperand; correctness wholly depends on each target hook to reject tied folds.

## worker-27  2026-05-21
- File: llvm/lib/Transforms/Vectorize/VectorCombine.cpp — focused on the listed fold targets: scalarizeOpOrCmp (1295), foldExtractedCmps (1466), foldShuffleOfBinops (2552), foldShuffleOfCastops (2803), foldShuffleOfShuffles (2935), foldSelectShuffle (4928), foldShuffleFromReductions (3792), foldBitcastShuffle (1078), foldInsExtFNeg (714), foldInsExtBinop (802).
- Patterns ruled out:
  - foldExtractedCmps predicate handling — `CmpPredicate::getMatching` correctly accounts for samesign and rejects fp-vs-int mismatches; new vcmp uses the matched predicate, so transforms are refinement-correct even under samesign-flip.
  - scalarizeOpOrCmp poison/flag propagation — `simplifyBinOp` over the constant operands is invoked without flags, but any introduced poison-in-defined-lane mismatch is in the refinement direction (poison can be refined to a defined value) so not a miscompile.
  - foldShuffleOfBinops div/rem poison guard (2570-2572) correctly bails when OldMask has poison lanes for div/rem; SameBinOp flag-handling is a no-op intersection (correct).
  - foldShuffleFromReductions (3792) — sort with `(unsigned)` comparator pushes PoisonMaskElem to the end as documented; all listed reductions are commutative+associative so reordering is sound; poison is still poison after reduction.
  - foldInsExtFNeg off-by-one at 741 (`ExtIdx > NumSrcElts`) allows ExtIdx == NumSrcElts but the resulting SrcMask value lands in [0, 2*NumSrcElts) (lane-0-of-second-poison-source), so still a valid mask producing poison; matches original OOB-extract poison semantics.
- Potential bugs filed:
  - candidates/w27-foldShuffleOfShuffles-bool-cast-of-PoisonValue.md — line 3022-3023 `return PoisonValue::get(ShuffleDstTy);` inside a `bool`-returning function: the PoisonValue* implicitly converts to true, and `replaceValue(I, ...)` is never called. The fold path claims success (analyses invalidated, debug log emitted) while the IR is left unchanged; introduced by 10756d32f (2026-05-16). Confirmed locally with `opt -passes=vector-combine -S`.

## worker-25  2026-05-21
- File: llvm/lib/CodeGen/CodeGenPrepare.cpp — read splitMergedValStore (8568-8665), splitBranchCondition (9314-9500), optimizeSelectInst + sinkSelectOperand (7587-7910), optimizeShiftInst/optimizeFunnelShift (7656-7722), optimizeLoadExt (7448-7583), sinkCmpExpression (1874-1945), foldICmpWithDominatingICmp (1966-2031), sinkAndCmp0Expression (2300-2369), OptimizeExtractBits/SinkShiftAndTruncate (2485-2568), foldURemOfLoopIncrement (2186-2262), unfoldPowerOf2Test (1803-1866), optimizeGatherScatterInst (6292-6424), combineToUAddWithOverflow/replaceMathCmpWithIntrinsic (1584-1729).
- Patterns ruled out:
  - sinkSelectOperand isSafeToSpeculativelyExecute correctly rules out side-effecting / atomic / volatile operands.
  - sinkCmpExpression and OptimizeExtractBits insert at FirstInsertionPt of UserBB; the sunk shift/cmp's operands transitively dominate UserBB via SSA, so dominance holds.
  - foldICmpWithDominatingICmp checks Cmp users are only CondBr or Select-with-cmp-as-condition; getSwappedPredicate + swapSuccessors/swapValues is direction-correct.
  - splitBranchCondition: Cond1/Cond2/LogicOp m_OneUse checks guard the move; PHI updates in TBB/FBB handle the new TmpBB edge correctly.
  - optimizeGatherScatterInst: getSplatValue returns either constant element or insertelement scalar that dominates the original shuffle, hence the gather/scatter.
  - unfoldPowerOf2Test: vector splat case handled by ConstantInt::get(VectorType, ...) returning a splat ConstantVector.
- Potential bugs filed:
  - candidates/w25-splitMergedValStore-atomic-not-checked.md — CGP splitMergedValStore bails on `isVolatile()` but never on `isAtomic()`; an atomic i64 store with the (or (zext lo), (shl (zext hi), 32)) value pattern is silently split into two non-atomic CreateAlignedStore halves, destroying atomicity and dropping the release/acquire ordering. Fires on x86 because X86TargetLowering::isMultiStoresCheaperThanBitsMerge returns true for the (int, FP) mix.

## worker-26 2026-05-21
- File: llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp:478-700 (foldCtpop/foldCtlz/foldCttz), 1232-1322 (matchSAddSubSat), 1590-1770 (bitreverse/bswap helpers, min/max-via-cmp), 2031-2350 (abs, umin/umax/smin/smax including xor-pow2 and NegPow2 mask folds), 2403-2625 (bswap, fshl/fshr), 2696-2820 (with-overflow common, ssub_with_overflow, uadd/sadd/usub/ssub_sat reassoc), 2821-3055 (minnum/maxnum/minimum/maximum/minimumnum/maximumnum, matrix_multiply, fmuladd, fma), 3056-3140 (copysign, fabs), 3996-4220 (vector_reduce_add/xor/mul/umin/umax/smin/smax i1 folds), 4237-4310 (is_fpclass, fptoui_sat, frexp, get_active_lane_mask, get_vector_length).
- File: llvm/lib/Transforms/InstCombine/InstCombineSimplifyDemanded.cpp:984-1180 (abs/ctpop/bswap/ptrmask/fshl/fshr/umax/umin SimplifyDemandedBits), 2191-2370 (simplifyDemandedFPClassMinMax / fneg(fabs) / FMin/FMax FP-class), 2820-2890 (intrinsic FP-class dispatch).
- Patterns ruled out:
  - vector_reduce_add/xor/umin/umax/smin/smax for sext/zext of <n x i1>: per-case verified by truth tables for n-lane parity (xor=add via mod2), AND/OR vs sext/zext.
  - cttz(sext(X)) -> cttz(zext(X)) is safe regardless of is_zero_undef because trailing-zero count is identical for both (low bits equal, X==0 case yields same BW result).
  - cttz(zext(X)) -> zext(cttz(X)) correctly gated by `match(Op1, m_One())` (is_zero_poison=true) so X=0 / poison cases are refinement-compatible.
  - fshl/fshr SimplifyDemandedBits with ShiftAmt = SA.urem(BitWidth) handles ShiftAmt==0 via APInt shl/lshr-by-BitWidth = 0; LHS/RHS demanded mask derivations match the fshl(L,R,S) = (L<<S) | (R>>(BW-S)) decomposition.
  - fma fneg(x) fneg(y) z -> fma x y z is mathematically equivalent (sign-bit XOR of NaNs is unspecified per LangRef, so allowed).
  - fma x -1.0 y -> fsub y x: single-rounded equivalence holds; -0/NaN signs not specified by LangRef.
  - smax/smin/umax/umin with NegPow2C-mask fold (line 2308) verified across positive/negative/wrap edge cases.
  - max(X,-X) / min(X,-X) -> fabs/-fabs fold is consistent with LangRef minnum/maxnum -0/+0 selection ("-0.0 considered smaller than +0.0").
  - matrix_multiply (-A)*B -> A*(-B) cost-based rewrites are mathematically NFC; element-count comparisons are well-formed.
  - ssub_with_overflow X,C -> sadd_with_overflow X,-C correctly gated by `C->isNotMinSignedValue()`.
  - bswap(shl X, Y) -> lshr(bswap X, Y) when low 3 bits of Y are known zero, verified by byte-position math.
  - cttz(add(lshr(UINT_MAX, X), 1)) -> sub(BW, X) is correct for X in [0, BW); X >= BW is poison-refinement.
- Potential bugs filed:
  - candidates/w26-vector-reduce-mul-sext-i1-odd-lanes.md — **CONFIRMED WRONG CODE** at InstCombineCalls.cpp:4112-4137. `vector_reduce_mul(sext <n x i1> V to <n x iM>)` is unconditionally folded to `zext(and-reduce(V))`, ignoring the sign-extension and the parity of `n`. For odd n with all-true V, the true product is `(-1)^n = -1`, but the fold yields `1`. Reproduced with `opt -passes=instcombine`: the function body becomes `bitcast V to i3 / icmp eq -1 / zext to i8`, returning `1` for `<true,true,true>` vs. the correct `i8 -1` confirmed by `opt -passes=instsimplify` on the all-constant version.

## worker-29 2026-05-21
- File: llvm/lib/Transforms/InstCombine/InstCombineCompares.cpp — spot-read:
  - foldICmpShrConstConst (962-1017), foldICmpShlConstConst (1021-1056)
  - processUGT_ADDCST_ADD (1065-1146): CI1 bitwidth-eq bail filed below
  - foldICmpWithConstant (1321-1368), foldICmpWithDominatingICmp (1371-1444)
  - foldICmpTruncConstant (1447-1530) — nuw skip-AND path verified safe
  - foldICmpAddConstant (3136-3293) — nsw/nuw split via getPreferredSignedPredicate verified
  - foldICmpInstWithConstantAllowPoison (3957-3976) — fshl/fshr rotate to X==0/-1 verified
  - foldICmpWithMinMax (5672-5815) — dropSameSign at line 5703 with comment is correct;
    EQ/NE branch logic with `(Pred == EQ) == *CmpXZ` semantically correct
  - foldICmpWithClamp (5825-5854), foldICmpPow2Test (~5857)
  - sign-bit canonicalization at 7127-7136 (Op0Known.Zero.isNegative()) verified
- File: llvm/lib/Transforms/InstCombine/InstCombineCasts.cpp — spot-read:
  - visitTrunc (978-1180) incl. `trunc nuw/nsw (xor X,Y) to i1 -> X!=Y` (1075-1079)
    and `trunc (lshr X, BW-1) to i1 -> icmp slt X, 0` (1044-1048) verified
  - narrowFunnelShift (737-836) — rotation-vs-funnel L=Width edge case checked safe
  - narrowBinOp (841-916) — flag-drop on Add/Sub/Mul rebuild conservative-correct
  - visitSExt (1799-1959), visitZExt entry, visitFPTrunc (2134-2319),
    visitFPExt (2321-2333), foldItoFPtoI (2340-2402), foldFPtoI (2408-2418),
    visitFPToUI / visitFPToSI / visitUIToFP / visitSIToFP (2420-2459),
    visitIntToPtr / visitPtrToInt (2462-2536)
- File: llvm/lib/Transforms/InstCombine/InstCombineAndOrXor.cpp — spot-read:
  - foldXorOfICmps (4725-4849), and-or-icmps codepath at 3380-3401
  - `IsSigned = LHS->isSigned() || RHS->isSigned()` with `predicatesFoldable` gate
    (CmpInstAnalysis.cpp:58) verified: signed/equality mix produces valid Code
- File: llvm/lib/Analysis/CmpInstAnalysis.cpp (1-100) — getICmpCode / getPredForICmpCode
  / predicatesFoldable inspected; encoding (UGT=001, EQ=010, ULT=100, NE=101, ULE=110)
  is symmetric across signed/unsigned so the IsSigned=OR choice in InstCombine is sound.
- File: llvm/include/llvm/IR/Operator.h:349-380 — confirmed FPMathOperator covers
  FAdd/FSub/FMul/FDiv/FRem/FNeg/FPTrunc/FPExt/FCmp/PHI/Select/Call but NOT SIToFP/UIToFP.
- File: llvm/lib/IR/Instructions.cpp:4043-4045 — getPreferredSignedPredicate semantics
  (returns signed only when samesign).
- Patterns ruled out:
  - foldICmpShrConstConst AShr negative-AP1/AP2 sign-match (981-984) correct.
  - foldICmpTruncConstant `Trunc->hasNoUnsignedWrap()` skip-AND (1493-1502): safe
    because nuw on trunc requires high bits zero, so wide-domain icmp is equivalent.
  - foldFPtoI fcPosNormal-vs-fcNormal asymmetry is poison-refinement under LangRef
    (negative normals are out-of-range for fptoui, so 0 is valid in poison sense).
    Filed as sanitizer-defeating note.
  - narrowFunnelShift rotation L=Width edge: rot(X, W) wide = X (high|low) which
    truncates back to X; matches fshl(X, X, 0) = X.
  - foldICmpWithMinMax `Pred = Pred.dropSameSign()` at line 5703 has the matching
    comment and decomposition reasoning; FoldIntoCmpYZ produces Y-Z compare without
    samesign which is correct because we cannot prove the recovered cmp is samesign.
  - foldXorOfICmps signbit-test fold (4751-4763) one-use gate, getInversePredicate
    correctness verified.
- Potential bugs filed:
  - candidates/w29-fpext-of-sitofp-drops-fmf.md — visitFPExt sinks FPExt through
    (s|u)itofp via CastInst::Create which drops FMF; SIToFP/UIToFP are not
    FPMathOperators so the FMF is silently lost.
  - candidates/w29-foldFPtoI-mask-fcposnormal-asymmetric.md — fcPosNormal mask for
    FPToUI vs fcNormal for FPToSI: poison-refinement that defeats fcastovf
    sanitizer for negative-normal-only paths.
  - candidates/w29-processUGT-ADDCST-CI1-bitwidth-eq-bails-too-eagerly.md —
    `CI1->getBitWidth() == NewWidth ||` bail in processUGT_ADDCST_ADD is overly
    conservative; missed-fold and fragile-guard, not a miscompile.
- No confirmed wrong-code in the assigned function set.

## worker-28 2026-05-21
- File: llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp:14200-14284, 14975-15130, 17088-17120, 17400, 18680-18700, 22587-22603, 23040-23130, 27800-28060, 28180-28310, 28500-28980 — focused on FP reduction FMF handling, FMulAdd CombinedOp emission, cmp+select min/max recognition, reduction reorder safety (RdxFMF/IgnoreReorder), propagateIRFlags wrap-flag handling.
- File: llvm/lib/Analysis/ValueTracking.cpp:9302-9341 — canConvertToMinOrMaxIntrinsic (SLP caller).
- File: llvm/lib/Transforms/Utils/LoopUtils.cpp:1625-1643 — propagateIRFlags impl (intersects flags across VL).
- Patterns ruled out:
  - RdxFMF intersection (28518-28523) covers all reduction ops, correctly propagated to Builder.setFastMathFlags (28974).
  - matchAssociativeReduction (28204-28281) downgrades RK to Ordered if any inner FMax/FMin lacks nnan; per-op CurrentRK from isVectorizable(EdgeInst) on line 28258.
  - reorderBottomToTop IgnoreReorder condition (28848-28851) is conservative: IgnoreReorder=true means SKIP root reorder, and the four-way OR allows skipping for reassoc-FP, int, ReductionLimit>2 (default), or Ordered.
  - Mixed-reassoc fadd chain test: SLP correctly preserves sequential evaluation for non-reassoc fadd, vectorizes only the reassoc-tagged pair.
  - Select-based fmin via `select(olt(a,b), a, b)` without fast-math is vectorized as vector cmp+select, NOT replaced with @llvm.minnum (so no semantic break from NaN handling there).
  - propagateIRFlags drops wrap flags when MinBWs.contains(E) (line 23111), and createOp at 27953 passes IncludeWrapFlags=false for reduction op flags.
  - createOp SMax/SMin/UMax/UMin fallthrough to intrinsic creation (27916-27924) uses getMinMaxReductionIntrinsicOp which correctly maps Kind to umax/umin/smax/smin intrinsics.
- Potential bugs filed:
  - candidates/w28-fmuladd-cost-emit-mismatch.md — TreeEntry::FMulAdd is recognized in transformNodes and given a fmuladd-priced cost discount, but vectorizeTree has no FMulAdd emit case; output IR is plain vector fmul+fadd, no @llvm.fmuladd. Cost-model regression on -mattr=-fma; not a miscompile.

## worker-33 2026-05-21
Scope: JumpThreading.cpp, CorrelatedValuePropagation.cpp, IndVarSimplify.cpp, SCCP.cpp (per task; also assigned to w34 in queue).
- File: llvm/lib/Transforms/Scalar/JumpThreading.cpp
  - processImpliedCondition (1144-1208) — found bug: distinct freeze instructions
    with same operand treated as same value (lines 1180-1184). Comment says
    "exactly the same freeze instruction" but check only compares operands;
    FICond->hasOneUse() at line 1156 guarantees PBI->getCondition() is a
    different FreezeInst. Per LangRef, each freeze independently picks. Filed.
  - processBranchOnPHI / processBranchOnXOR / threadEdge - spot-checked, OK.
- File: llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp
  - processICmp (288-322) — signed->unsigned predicate swap via
    ConstantRange::getEquivalentPredWithFlippedSignedness; followed by samesign
    setting via areInsensitiveToSignednessOfICmpPredicate. Both monotonic in
    the right direction. OK.
  - processOverflowIntrinsic (638-662) — sets nuw/nsw using isSigned() flag of
    WithOverflowInst; only invoked if willNotOverflow returns true. OK.
  - processSaturatingInst (664-681) — same pattern, OK.
  - processSExt (1120-1136) — replaces with zext+nneg if LVI proves non-neg.
    Range checks UndefAllowed=false. OK.
  - processSRem (955-1007) / processSDiv (1014-1069) — operand-domain analysis
    via Domain enum; negate when NonPositive; URem/UDiv; result negation logic.
    Spot-check: SDiv preserves isExact correctly (line 1051); SRem has no isExact
    to preserve. OK.
  - processCmpIntrinsic / processMinMaxIntrinsic / processAbsIntrinsic - OK.
  - processBinOp (1179-1211) - sets nuw/nsw via deduced range; only ever sets,
    never clears. OK.
  - processTrunc (1236-1262) - nuw/nsw inference for trunc; OK.
- File: llvm/lib/Transforms/Scalar/IndVarSimplify.cpp
  - linearFunctionTestReplace (1060-1197) — nowrap flag adjustment at 1111-1117
    only ever DROPS (`setHasNoUnsignedWrap(AR->hasNoUnsignedWrap())` is monotonic
    when the original was already set). Comment at 1107-1110 acknowledges a known
    incomplete case for dynamically-dead IVs but doesn't actually miscompile.
  - genLoopLimit / needsLFTR / FindLoopCounter / isLoopCounter / hasConcreteDef
    - spot-read; hasConcreteDef explicitly handles undef paths conservatively. OK.
  - optimizeLoopExits (1657-1825) — uses SE->getExitCount and SE-guarded folds;
    the SkipLastIter UpdateSkipLastIter logic at 1723-1734 handles the
    iteration-minus-one corner case. Looks correct.
  - simplifyAndExtend / createWideIV path — delegated to SimplifyIndvar; not deeply
    audited here.
- File: llvm/lib/Transforms/Scalar/SCCP.cpp — just a driver (140 lines); real logic
  in Utils/SCCPSolver.cpp. Spot-checked SCCPInstVisitor::visitFreezeInst at
  SCCPSolver.cpp:1685-1707: gates on `isGuaranteedNotToBeUndefOrPoison` of the
  resolved constant before propagating, which is correct.
- Patterns ruled out:
  - SCCP visitFreezeInst observing frozen poison through const-propagation — gated
    correctly.
  - CVP overflow-flag flip on predicate swap — does not occur (with.overflow
    intrinsic rewrite is gated by willNotOverflow).
  - IndVarSimplify LFTR nowrap-flag inflation — code only narrows flags.
- Potential bugs filed:
  - candidates/w33-jumpthreading-implied-cond-distinct-freeze-same-operand.md —
    JumpThreading's processImpliedCondition treats two distinct `freeze i1 %v`
    instructions as having the same value; folds away a reachable successor.
    Transform-confirmed with `opt -passes=jump-threading` on /tmp/w33_jt_freeze2.ll.
    Runtime miscompile not constructed (depends on nondeterministic freeze pick).

## worker-34 2026-05-21
- File: llvm/lib/CodeGen/LiveVariables.cpp:1-887 — full read; focused on HandleRegMask largest-super-reg promotion (412-428), HandlePhysRegKill partial-use/dead/early-clobber paths (300-406), runOnBlock liveOut detection, addNewBlock PHI alive-block propagation.
- File: llvm/lib/CodeGen/LiveIntervals.cpp:140-300, 308-460 (computeRegMasks / computeRegUnitRange / computeLiveInRegUnits), 950-1120 (checkRegMaskInterference), 1027-1610 (HMEditor handleMove / handleMoveDown/Up / updateRegMaskSlots / findLastUseBefore / repairOldRegInRange), 1779-1820 (removePhysRegDefAt / removeVRegDefAt / splitSeparateComponents).
- File: llvm/lib/CodeGen/LiveIntervalCalc.cpp:90-194 — createDeadDefs / extendToUses (subrange undef accounting + early-clobber redef detection).
- File: llvm/lib/CodeGen/RegisterCoalescer.cpp:990-2000 (removeCopyByCommutingDef subrange merge, reMaterializeDef, eliminateUndefCopy, updateRegDefsUses), 2300-2415 (joinReservedPhysReg), 2730-3210 (JoinVals::computeWriteLanes/followCopyChain/valuesIdentical/analyzeValue/computeAssignment/taintExtent/usesLanes/resolveConflicts), 3380-3580 (pruneSubRegValues / pruneMainSegments / eraseInstrs CR_Keep branch).
- Patterns ruled out:
  - LiveVariables `HandleRegMask` (412-428) largest-clobbered-super-reg promotion: targets generate regmasks where a super reg is clobbered iff all its parts are; the few targets with partial-super-reg-mask regs handle this via the explicit super-reg-clobber bit; not exploitable.
  - LiveIntervals `computeRegUnitRange` (311-350) intentionally excludes regmask clobbers from regunit live ranges — the canonical pattern is that callers must combine `LR.liveAt()` with `checkRegMaskInterference`. Design, not bug.
  - LiveIntervals `handleMove::updateAllRanges` (1056-1120) only propagates regmask through `updateRegMaskSlots()` which just shifts the slot in `RegMaskSlots`; this is consistent because regmasks are not modeled in regunit ranges (see above).
  - LiveIntervals `findLastUseBefore` regunit branch (1527-1556) intentionally ignores regmasks — same rationale; regmask is a clobber, not a use.
  - LiveIntervals `removePhysRegDefAt` (1779-1785) only removes the VNI; no subrange cleanup needed because regunit ranges don't have subranges.
  - RegisterCoalescer `joinReservedPhysReg` (2386-2393) upward-hoist scan uses `MI->readsRegister(DstReg, TRI)` which correctly considers regunit aliases via TRI; regmask interference is separately covered at 2334.
  - RegisterCoalescer `reMaterializeDef` collects implicit physreg defs into `NewMIImplDefs` (1466-1495) and creates regunit dead defs for each at 1687-1692 — but only AFTER the partial-physreg liveness gate at 1349-1361 (see candidate below).
  - JoinVals `analyzeValue` line 3008 `valuesIdentical` correctly distinguishes undef-chain-source equality from real-value equality via `followCopyChain` returning `(nullptr, SrcReg)` for undef terminators.
  - JoinVals `resolveConflicts::taintExtent` (3152-3190) correctly bails when tainted lanes escape the BB; dead-def stop condition at 3175 is correct.
  - LiveIntervalCalc `extendToUses` (134-194) early-clobber-tied-def detection at 182-186 is correct.
- Potential bugs filed:
  - candidates/w34-rematerialize-partial-physreg-misses-implicit-def-liveness.md — `reMaterializeDef`'s partial-physreg liveness gate (1349-1361) scans only regunits of the explicit DstReg, missing live regunits clobbered by `DefMI`'s implicit physreg defs (e.g. `$eflags` from `MOV32r0`); the post-hoc createDeadDef loop at 1687-1692 then silently truncates the clobbered regunit's live range. Latent miscompile path on x86 RMW remat into partial-physreg COPY.

## worker-36 2026-05-21
- File: llvm/lib/CodeGen/StackProtector.cpp:1-784 — full read; focused on `InsertStackProtectors` tail-call hoist (671-674), noreturn-call CheckLoc selection (622-639), SP_return iteration safety (717-744), CreateFailBB.
- File: llvm/lib/CodeGen/ShrinkWrap.cpp:1-1059 — full read; focused on `useOrDefCSROrFI` SP/CSR/FI checks, `updateSaveRestorePoints` loop fixups, `postShrinkWrapping` DirtyBB collection (618-630), `checkIfRestoreSplittable` clean/dirty pred classification, `markAllReachable`, `tryToSplitRestore`/`rollbackRestoreSplit`, `performShrinkWrapping` RPO pred-stack-address propagation, `isShrinkWrapEnabled` sanitizer/WinCFI gates.
- File: llvm/lib/CodeGen/PrologEpilogInserter.cpp:1-1588 — full read; focused on `calculateSaveRestoreBlocks` (single save/restore from shrink-wrap, EH funclet entries → extra SaveBlocks), `assignCalleeSavedSpillSlots` (SavedSuper check 467-478), `updateLiveness` Save/Restore walk (542-620), `calculateFrameObjectOffsets` (CSR-first then SSP-protected then everything else), `insertZeroCallUsedRegs` reg-zero set construction, `replaceFrameIndices{,Backward}` SPAdj tracking, `computeMaxCallFrameSize` (recompute invariant).
- File: llvm/lib/CodeGen/StackColoring.cpp:1-1381 — full read; focused on `collectMarkers` BetweenStartEnd / ConservativeSlots / SeenStartMap (628-781), `calculateLocalLiveness` LiveOut = LiveIn − End + Begin (783-837), `calculateLiveIntervals` Starts vs LiveStarts (839-898), merge predicate `!First.isLiveAtIndexes(SecondS) && !Second.isLiveAtIndexes(FirstS)` (1337-1338), `remapInstructions` MMO/AA updates, `expungeSlotMap` path compression.
- Patterns ruled out:
  - PEI miscomputes stack size per CC: `assignCalleeSavedSpillSlots` SavedSuper check uses BOTH `SavedRegs.test(SuperReg) && CSMask.test(SuperReg)`, so an aliased super-reg that isn't in this CC's CSR list cannot wrongly suppress a sub-reg slot. `computeMaxCallFrameSize` invariant `MaxCallFrameSize <= MaxCFSIn` is asserted on recompute (line 395-396).
  - ShrinkWrap "prologue into block whose preds include unreachable edge": `postShrinkWrapping`'s DirtyBB collection skips `!MDT->isReachableFromEntry` (line 619), and `markAllReachable` only follows successors of reachable dirty blocks (which are themselves reachable). An unreachable `U → CurRestore` edge gets classified as a CleanPred but is unreachable so never executes; the saved `NewSave` dominates only DirtyPreds, the prologue is never skipped for any executable path. `isSaveReachableThroughClean` walks pred chains and would refuse the split if a clean path bypasses NewSave.
  - StackColoring lifetime-marker overlap merge: per-block BlockInfo correctly cancels `start;end` pairs (Begin reset on End — lines 749-755), `start;end` in same block correctly emit a bounded segment [start, end] without leaving the slot live-out; the LiveStarts vector deliberately omits LiveIn-introduced indices because the overlap predicate `T.isLiveAt(starts_of_S) ∨ S.isLiveAt(starts_of_T)` is symmetric and captures the overlap via the other slot's start. Verified by case analysis on patterns `start S; start T; end S; end T` and chained CFG cases.
  - StackProtector SP_return iteration recursion concern: `SupportsSelectionDAGSP` is true for X86 by default (EnableSelectionDAGSP=true && !FastISel), so the loop `break`s after the first IR-level prologue insertion; the recursive-instrument-SP_return scenario only arises under -fast-isel + ssp-strong, where `useStackGuardXorFP()` is the gate — for X86 this is false but `make_early_inc_range` does capture next-iterator before reorder.
- Potential bugs filed:
  - candidates/w36-stackprotector-tailcall-intervening-instr.md — `InsertStackProtectors` tail-call hoist only walks one instruction back from CheckLoc; misses `sext`/`zext`/`bitcast`/`freeze`/`gep` and lifetime/assume intrinsics that `isInTailCallPosition` permits between the tail call and the ret. X86 backend's conservative tail-call gating hides this today, but musttail and other backends (AArch64/ARM) can expose it.

## worker-32 2026-05-21
- File: llvm/lib/Transforms/Vectorize/LoopVectorize.cpp:1180-1265 (TailFoldingStyle selection + usePredicatedReductionSelect), 2363-2456 (isScalarWithPredication / isPredicatedInst — store/load/udiv/sdiv predication-required rules under tail folding), 2575-2608 (interleave-group masking), 2622-2738 (uniform memop scalarization rules), 2877-3076 (computeMaxVF + tail-folding fallback chain), 4622-4810 (setCostBasedWideningDecision uniform/gather/scatter/interleave choice), 6280-6348 (tryToWidenMemory recipe builder — reverse access, GEP nuw drop under tail-fold), 6800-7140 (createInitialVPlan tail of pipeline: addReductionResultComputation, AnyOf reduction rewrite at 7014-7060, ComputeReductionResult+ReductionStartVector at 7060-7135), 7595-7637 (epilogue resume value for AnyOf/FindIV).
- File: llvm/lib/Transforms/Vectorize/VPlanTransforms.cpp:1064-1192 (tryToComputeEndValueForInduction + optimizeInductionLiveOutUsers: end value = TripCount under FoldTail).
- File: llvm/lib/Transforms/Vectorize/VPlanRecipes.cpp:840-875 (VPInstruction::AnyOf execute path: Freeze + OrReduce).
- Patterns ruled out:
  - tail-folded UDiv/URem with loop-invariant divisor (LoopVectorize.cpp:2447): scalar loop's nonzero divisor invariant transfers to vector loop correctly.
  - tail-folded load with invariant pointer (2436) and store with invariant ptr+value (2442): unmasked execution writes the same value as scalar would — semantically equivalent.
  - Reverse-consecutive GEP under tail-fold (6306-6316) correctly drops nuw flag.
  - optimizeInductionLiveOutUsers using `Plan.getTripCount()` for tail-folded loops (VPlanTransforms.cpp:1151-1152): correct — exit-block IV must equal scalar-loop end IV, not vector-trip-count multiple.
  - Truncated-then-extended reductions (7070-7097) correctly route Trunc/Extnd through ComputeReductionResult operand 0.
- Potential bugs filed:
  - candidates/w32-anyof-reduction-tail-fold-no-mask-on-cmp.md — AnyOf reduction rewrite at LoopVectorize.cpp:7036-7040 creates `Or(PhiR, Cmp)` without AND-ing Cmp with HeaderMask; under tail folding, `Cmp` on inactive lanes is computed from poison (masked-load result), and the downstream `freeze` in VPInstruction::AnyOf can pick a `true` bit, flipping the reduction answer.

## worker-35 2026-05-21
- File: llvm/lib/CodeGen/BranchFolding.cpp:1-2172 — focused read; ProfitableToMerge / ComputeCommonTailLength / mergeOperations / mergeCommonTails / CreateCommonTailOnlyBlock / RemoveBlocksWithHash / replaceTailWithBranchTo / SplitMBBAt / OptimizeBlock (MBB-into-PrevBB splice, tail-call conditionalization) / HoistCommonCodeInSuccs / findHoistingInsertPosAndDeps.
- File: llvm/lib/CodeGen/TailDuplicator.cpp:1-1105 — focused read; shouldTailDuplicate, canTailDuplicate, duplicateSimpleBB, tailDuplicate main path, duplicateInstruction (CFI special case + vreg remap), updateSuccessorsPHIs, appendCopies.
- File: llvm/lib/CodeGen/IfConversion.cpp:1-2372 — focused read; ScanInstructions / RescanInstructions / FeasibilityAnalysis / CountDuplicatedInstructions / MaySpeculate / PredicateBlock / CopyAndPredicateBlock / MergeBlocks. (X86 has no isPredicable instructions, so the predication path is dead on x86 — only the branch-elimination path fires.)
- Patterns ruled out:
  - mergeOperations: isIdenticalTo() compares all operands incl. implicit-defs (same opcode -> same desc -> same implicit operand count), and ComputeCommonTailLength bails on first mismatch — so the "differ in implicit-defs" path can't fire for same-opcode MIs.
  - HoistCommonCodeInSuccs: findHoistingInsertPosAndDeps adds the JCC's EFLAGS to `Uses` and (when PI is the flag-setter) to `Defs`, so an arm-instruction defining EFLAGS hits the Uses-overlap bail (line 2018) and an arm-instruction reading EFLAGS hits the Defs-overlap bail (line 2040) — both directions safe.
  - mergeOperations undef-flag drop is followed by computeLiveIns + per-pred IMPLICIT_DEF insertion (lines 875-905) when UpdateLiveIns is true (always true on x86 after RA).
  - replaceTailWithBranchTo also inserts IMPLICIT_DEFs for newly-live-in regs (line 405-417).
  - TailDuplicator canTailDuplicate: PredBB->succ_size() > 1 bails for invoke preds (EH edges count), so EH-pad TailBBs aren't duplicated through invoke arms.
  - TailDuplicator shouldTailDuplicate bails on isNotDuplicable / isConvergent / INLINEASM_BR / PreRegAlloc-isReturn / PreRegAlloc-isCall.
  - IfConversion ScanInstructions: inline asm (INLINEASM/INLINEASM_BR) sets BBI.IsUnpredicable via the `!TII->isPredicable(MI)` check (line 1164), and the shared-portion ClobbersPredicate check in CountDuplicatedInstructions (line 766) returns false on a pred-clobber.
  - IfConversion MaySpeculate (line 2110): rejects an instruction whose def is not in LaterRedefs — implicit EFLAGS defs are covered.
  - On x86 IfConversion is effectively a no-op for predication because no X86 InstrDesc has the isPredicable bit set; the only thing that runs is the branch-elimination/merge tail of the pass.
- Potential bugs filed:
  - candidates/w35-branchfold-optblock-merge-loses-eh-scope-entry.md — OptimizeBlock MBB-into-PrevBB splice (line 1450-1476) checks `!MBB->isEHPad()` but not isEHScopeEntry / isEHFuncletEntry; if the latter aren't subsets of EHPad, splicing strips the marker. Source-level; needs WinEH repro.
  - candidates/w35-branchfold-ehscope-empty-skips-itanium-check.md — ProfitableToMerge (line 581-589) skips the cross-EH-scope check whenever EHScopeMembership is empty. The map is empty for Itanium DWARF EH (the default Linux target) because getEHScopeMembership early-returns when !MF.hasEHScopes(). Two tail blocks from different invoke regions can in principle be merged; EH_LABEL pseudos usually act as a natural barrier in ComputeCommonTailLength but the safeguard is structural, not semantic. Filed for fuzzer follow-up.

## worker-31 2026-05-21
- File: llvm/lib/Transforms/Scalar/GVN.cpp:1-3410 — focused read; covered Expression struct (operator==/hash_value), createExpr/createGEPExpr/createExtractvalueExpr, ValueTable::lookupOrAddCall/computeLoadStoreVN/addMemoryStateToExp, AnalyzeLoadAvailability (atomic ordering checks), findDominatingValue (select-pointer load forwarding), eliminatePartiallyRedundantLoad (PRE load alignment/ordering preservation), PerformLoadPRE block-walk, processLoad/processNonLocalLoad isUnordered guards, processMaskedLoad (m_MaskedStore pattern, Dep.isDef must-alias), processAssumeIntrinsic, performScalarPRE/performScalarPREInsertion (flag preservation via patchReplacementInstruction → andIRFlags), lookupOrAdd(MemoryAccess).
- File: llvm/lib/Transforms/Scalar/EarlyCSE.cpp:1-1977 — focused read; SimpleValue::canHandle (constrained-FP rounding/exception bailouts, freeze in handled set), getHashValueImpl/isEqualImpl (BinOp/Cmp commutation, min/max recognition, FreezeInst hash by opcode+op0, isIdenticalToWhenDefined with IntersectAttrs), CallValue::isEqual (convergent bb check), GEPValue::isEqual (constant-offset shortcut, pointer-operand check), combineIRFlags (FP-flag intersection, GEP-inbounds intersect for hasPoisonGeneratingFlags && !programUndefinedIfPoison), ParseMemoryInst (isVolatile/isAtomic/isUnordered), getMatchingValue (volatile/atomic guards lines 1261-1265), writeback-DSE (line 1728), Release-fence skip generation bump (line 1717), LastStore tracking unordered-only.
- File: llvm/lib/Transforms/Scalar/NewGVN.cpp:1-4292 — focused read; performSymbolicLoadCoercion (LI->isSimple precondition + LI->isAtomic > DepSI->isAtomic guard), performSymbolicLoadEvaluation (early bail on !isSimple), createBinaryExpression / select path in performSymbolicEvaluation (FMF handling), performSymbolicCallEvaluation (doesNotAccessMemory + onlyReadsMemory + convergent + coroutine bailouts), setMemoryClass.
- Patterns ruled out:
  - GVN load PRE preserves isVolatile/getAlign/getOrdering/getSyncScopeID when cloning into predecessor (line 1574).
  - GVN load forwarding from store/load uses `Load->isAtomic() <= DepX->isAtomic()` (lines 1339/1356/1419/1434) — correct direction (never forward weaker into stronger).
  - GVN processLoad/processNonLocalLoad gated on `L->isUnordered()` (line 2163); release/acquire/seqcst loads are never touched.
  - GVN masked-load → masked-store forwarding (line 2225) relies on Dep.isDef must-alias from MemDep; mask must `m_Specific` match and value type must match.
  - EarlyCSE getMatchingValue refuses to forward when MemInst is volatile or non-unordered (line 1261), and refuses to drop an atomic load whose dep was non-atomic (line 1264).
  - EarlyCSE GEP CSE intersects flags via combineIRFlags(Inst, V) (line 1697) which strips inbounds from surviving GEP when the new one lacks it (programUndefinedIfPoison gate).
  - EarlyCSE FreezeInst CSE is OK: two `freeze X` with same X may legitimately produce the same arbitrary value.
  - NewGVN performSymbolicLoadCoercion checks `LI->isSimple()` (assert + caller bails for non-simple); store→load type-match shortcut is fine.
  - NewGVN call VN excludes convergent/coroutine cases (lines 1646-1653).
- Potential bugs filed:
  - candidates/w31-gvn-mssa-computeLoadStoreVN-ignores-atomic-volatile.md — under `-enable-gvn-memoryssa`, the load/store Expression key omits isVolatile/isAtomic/getOrdering/getSyncScopeID; reproduced atomic-load eliminating subsequent non-atomic load via `opt -enable-gvn-memoryssa -passes=gvn`. The direction observed is benign (stronger replaces weaker, IsUnordered guard blocks the reverse), but the VN invariant violation is a latent footgun for any future code path that uses VN equivalence without re-checking `isUnordered()`.
  - candidates/w31-newgvn-simplifySelectInst-drops-fmf.md — NewGVN passes empty `FastMathFlags()` to `simplifySelectInst` (line 1216) and uses the non-FMF `simplifyBinOp` overload (lines 1120/1222); missed-FP-simplification only (always-pessimistic, never unsound).

## worker-46 2026-05-21
- Files: llvm/lib/CodeGen/AsmPrinter/AsmPrinter.cpp:1-5337 (focused: emitGlobalVariable, computeGlobalGOTEquivs, isGOTEquivalentCandidate, emitGlobalGOTEquivs, getFunctionCFISectionType, emitCFIInstruction, needFuncLabels); llvm/lib/CodeGen/AsmPrinter/DwarfCFIException.cpp:1-155 (full); llvm/lib/CodeGen/PseudoProbeInserter.cpp:1-153 (full). MachineCFGPrinter.cpp skipped per instructions.
- Patterns ruled out:
  - DwarfCFIException::beginFunction null-personality path is safe: shouldEmitPersonality requires `Per` not null, and beginBasicBlockSection's `assert(P)` only fires when shouldEmitPersonality is true (so Per/P is GlobalValue).
  - classifyEHPersonality itself stripPointerCasts (EHPersonalities.cpp:25), so AsmPrinter.cpp:1957 passing raw `getPersonalityFn()` is fine.
  - PseudoProbeInserter reverse-iteration move loop: `MII = MBB.rbegin(); auto Cur = MII++` then `remove(&*Cur); insert(FirstInstr, ProbeInstr)` is safe — the moved probe is inserted before FirstInstr, so the reverse walk hits FirstInstr (non-pseudo) and breaks before re-encountering the moved probe.
  - emitCFIInstruction MBB->end() vs MBB->instr_end() distinction: identical (MachineBasicBlock.h line 386: `end() { return instr_end(); }`).
- Potential bugs filed:
  - candidates/w46-asmprinter-got-equiv-skips-tls-check.md — `isGOTEquivalentCandidate` (AsmPrinter.cpp:2550) does not exclude `thread_local` globals. A constant unnamed_addr TLS global pointing to another GV gets added to `GlobalGOTEquivs`; with `-femulated-tls` the EmuTLS early return at line 793 fires twice (initial + emitGlobalGOTEquivs reemit) leaving the symbol undefined. Source-level; needs targeted IR for repro.
  - candidates/w46-asmprinter-emitCFI-skip-uses-MF-rbegin-not-isEndSection.md — emitCFIInstruction (AsmPrinter.cpp:1432) compares `MBB->getReverseIterator() == MBB->getParent()->rbegin()` to detect "last MBB" for FDE-end trimming. With `-basic-block-sections` / HotColdSplit / DwarfCFI funclets, a function has multiple FDEs; the check misses end-of-section MBBs that are not end-of-function, so a trailing CFI escapes into the next section's FDE. Should use `MBB->isEndSection()`. Matches "Wrong CFI cancel/restore emission across funclets" pattern.

## worker-47 2026-05-21
- Focus: x86 `-global-isel` path; CombinerHelper / LegalizerHelper / IRTranslator.
- File: llvm/lib/CodeGen/GlobalISel/CombinerHelper.cpp:239-3030 — scanned all
  `bool CombinerHelper::match*` declarations (60+). Focused on undef/store/load
  matchers, matchEqualDefs (loads/stores guarded by isDereferenceableInvariantLoad
  and size compare; falls through to isIdenticalTo so addresses get checked),
  matchInsertExtractVecEltOutOfBounds (bails on scalable, OK).
- File: llvm/include/llvm/Target/GlobalISel/Combine.td:430-720 — cross-checked
  apply rules for the undef matchers. undef_to_fp_zero / undef_to_int_zero /
  undef_to_negative_one / propagate_undef_any_op / propagate_undef_all_ops all
  represent legal-but-aggressive refinements of undef.
- File: llvm/lib/CodeGen/GlobalISel/LegalizerHelper.cpp:1539-1820,
  6098-6535 (narrowScalar dispatch + shift narrowing), 7100-7720 (multiplyRegisters,
  narrowScalar{AddSub,Mul,Insert,Extract,Basic,Ext,Select,CTLZ,CTTZ,CTLS,CTPOP,FLDEXP,FPTOI}).
- Patterns ruled out:
  - `narrowScalarMul` dispatch only covers G_MUL and G_UMULH (no G_SMULH); falls
    back to UnableToLegalize so not a miscompile, at most a missing fallback —
    targets that mark G_SMULH narrow are responsible to provide their own. Not a
    bug.
  - `multiplyRegisters` always uses unsigned UMULH for high parts: correct for
    both G_MUL low-half and G_UMULH (the top NumParts of the 2*NumParts result
    is the unsigned-high half).
  - `narrowScalarAddSub`: for SADDO/SSUBO with >1 parts the OpF=SADDE/SSUBE is
    correctly only used at i==e-1 (signed overflow happens at MSB).
  - `narrowScalarCTLZ`/`CTTZ`: the unconditional `_ZERO_POISON` use on the
    "other half" is dead — select picks the other arm precisely when the
    poison-input branch would be invoked.
  - IRTranslator.cpp: no i128/fp128/vector-of-pointer special-case code (handled
    generically via LLT). Did not find a translation-side bug in spot-checks of
    translateGetElementPtr / translateInsertVector / translateExtractVector /
    translateShuffleVector / translateVectorInterleave2Intrinsic.
- Potential bugs filed:
  - candidates/w47-gisel-matchUndefStore-drops-volatile-atomic.md — confirmed
    miscompile (asm-level). `CombinerHelper::matchUndefStore` (line 2890) only
    checks the value operand is G_IMPLICIT_DEF; combine rule `erase_undef_store`
    (Combine.td:715-720) unconditionally erases the G_STORE. Volatile stores and
    atomic stores of undef are silently dropped on x86 with `-global-isel`. DAG
    ISel correctly emits `movl`/`xchgl`; GISel emits only `retq`.

## worker-45 2026-05-21
- File: llvm/lib/Target/X86/X86AsmPrinter.cpp:1-1194 — full read; focused on PrintAsmOperand/PrintAsmMemoryOperand (modifiers a/A/c/p/P/n/V/b/h/w/k/q/x/t/g), PrintMemReference / PrintIntelMemReference / PrintLeaMemReference, PrintSymbolOperand target-flag table.
- File: llvm/lib/Target/X86/MCTargetDesc/X86IntelInstPrinter.cpp:1-497 — full read; printVecCompareInstr (CMP/VCMP/VPCMP/VPCOM with broadcast/EVEX_B/EVEX_K branches), printMemReference, printSrcIdx/printDstIdx (hardcoded "es:"), printOptionalSegReg use at Op+1.
- File: llvm/lib/Target/X86/MCTargetDesc/X86ATTInstPrinter.cpp:1-541 — full read; same VCMP CurOp-walking logic (post-decrement chain), AT&T printMemReference, printDstIdx hardcoded "%es:(".
- File: llvm/lib/Target/X86/AsmParser/X86AsmParser.cpp:3050-3082 (CheckDispOverflow), 3878-4105 (processInstruction, validateInstruction), 4125-4161 (applyLVICFIMitigation), 4380-4500 (MatchAndEmit), 4625-4677 (push-immediate isIntN/isUIntN OR + unsized-mem loop).
- Patterns ruled out:
  - IntelInstPrinter `printDstIdx` hardcoding `"es:"` matches Intel-syntax convention for STOS/MOVS/SCAS destinations (ES is fixed, not overridable); same in AT&T (`%es:(...)`).
  - VCMP/VPCMP CurOp post-decrement bookkeeping in both printers consistent across EVEX_K and broadcast forms.
  - CCMP/CTEST replacement (processInstruction) uses CC=10 ("t" / true) matching `defm : CCMP_Aliases<"t" ,10>` in X86InstrAsmAlias.td.
  - CheckDispOverflow's warning-vs-error split between 64-bit (error if !isInt<32>) and 32/16-bit (warn-and-truncate via isUIntN) is intentional gas-compat behavior, not a bug.
- Potential bugs filed:
  - candidates/w45-asmprinter-modifier-a-A-intel-dialect.md — `%a` (case 'a') hardcodes `(%rip)` and `(reg)` AT&T punctuation, and `%A` hardcodes `*reg` indirection, regardless of `MI->getInlineAsmDialect()`. Intel-syntax inline asm using these constraint modifiers produces invalid output.
  - candidates/w45-asmprinter-P-modifier-att-intel-asymmetry.md — `%P` modifier's "disp-only" Modifier string is honored by `PrintIntelMemReference` (suppresses HasBaseReg) but completely ignored by `PrintLeaMemReference`, so AT&T output still prints `(base,index)` parens. Wrong constraint-modifier output for AT&T users.
  - candidates/w45-asmparser-lvi-cfi-shl64-in-32-bit-mode.md — `applyLVICFIMitigation` hardcodes `X86::SHL64mi` even when matching RET16/RET32/RETI16/RETI32 in 16/32-bit modes; emits a REX.W-prefixed shift in a mode that forbids REX.

## worker-40 2026-05-21
- File: llvm/lib/CodeGen/WinEHPrepare.cpp:1-1448 — full read; focused on prepareExplicitEH/colorFunclets/cloneCommonBlocks (CloneBasicBlock preserves metadata + RemapInstruction remaps metadata via VMap; not a metadata-loss source), demotePHIsOnFunclets, removeImplausibleInstructions (IsUnreachableRet/Catchret/Cleanupret), replaceUseWithLoad catchret edge-split (catchret/goto swap + new color = destination), calculateCXXStateNumbers + IsPreOrder gating on isArch64Bit, calculateClrEHStateNumbers TryParentState inference, getCleanupRetUnwindDest first-match.
- File: llvm/lib/Target/X86/X86AsmPrinter.cpp:1-1194 — full read; emitKCFITypePadding/emitKCFITypeId, EmitFPOData lifecycle (line 89 set, 111 reset per fn), emitBasicBlockEnd SplitChainedAtEndOfBlock, emitMachOIFuncStub*, emitEndOfAsmFile COFF/MachO/ELF dispatch.
- File: llvm/lib/Target/X86/X86MCInstLower.cpp:1730-1823, 2540-2680, 2707-2870 — EmitSEHInstruction switch (FPO vs non-FPO branches), per-opcode handling, maybeEmitNopAfterCallForWindowsEH cross-MBB iteration.
- Patterns ruled out:
  - WinEHPrepare cloneCommonBlocks dropping metadata — CloneBasicBlock uses I.clone() + cloneDebugInfoFrom; RemapInstruction (ValueMapper.cpp:1018-1025) does remap attached metadata.
  - EmitSEHInstruction non-FPO switch missing SEH_StackAlign — only emitted from X86FrameLowering.cpp:1995 which is gated on !IsWin64Prologue and so always routes through the FPO branch; non-FPO unreachable is by construction.
  - SEH_BeginEpilogue / SEH_EndEpilogue missing from FPO branch — only emitted under NeedsWin64CFI which is mutually exclusive with EmitFPOData.
  - Win64 stack realignment AND not having a SEH directive — that AND is placed *after* SEH_EndPrologue and the unwinder uses FP, so it's unrepresented intentionally.
  - WinEHPrepare IsPreOrder gated on isArch64Bit (not personality/triple-OS) — benign because MSVC personality is what drives the consumer.
  - removeImplausibleInstructions cleanup-vs-MSVC_CXX restriction — intentional per-personality semantics, not a bug.
  - cloneCommonBlocks catchret edge-split coloring (replaceUseWithLoad path) — new block correctly takes destination (PHIBlock) colors, not source funclet colors.
- Potential bugs filed:
  - candidates/w40-asmprinter-coff-fltused-early-return-skips-morestack.md — X86AsmPrinter::emitEndOfAsmFile uses `return` (not `break`) after emitting _fltused on COFF when usesMSVCFloatingPoint is true; skips the trailing `__morestack_addr` emission for x86_64 + CodeModel::Large + split-stack. Asm-confirmed: with `fadd` the symbol definition vanishes; without it, the symbol is correctly emitted.

## worker-38 2026-05-21
- File: llvm/lib/Target/X86/X86InstrFragments.td:645-870 — read all C++ PatFrag bodies (loadi8/16/32/64, ext/sextloadi*, alignedloadf128, memopf128, binop_oneuse, X86add_flag_nocf, X86sub_flag_nocf, X86testpat, PrefetchWLevel, X86lock_*_nocf, X86tcret_enough_regs, anyext_sdiv, def32, shiftMask8/16/32/64).
- File: llvm/lib/Target/X86/X86InstrFragmentsSIMD.td:1260-1785 — read alignedload, memop, vextract128/256_extract, vinsert128/256_insert, masked_load{,_aligned}, masked_store{,_aligned}, X86mExpandingLoad, X86mCompressingStore, X86mtruncstore + per-element variants, X86Vfpclass*_su, vandn/vxnor, X86Vpshufbitqmb_su, X86pcmpgtm, X86pcmpm_imm{,_commute}, X86pcmpm/pcmpm_su/pcmpum/pcmpum_su, X86cmpm_imm_commute, X86cmpm_su, X86cmpms_su.
- File: llvm/lib/Target/X86/X86InstrCompiler.td:17-1900 (spot) — GetLo32XForm, mov64imm32 ComplexPattern, globalAddrNoAbsSym, BTRXForm/BTCBTSXForm, BTRMask64/BTCBTSMask64, immff00_ffff.
- File: llvm/lib/Target/X86/X86InstrShiftRotate.td:520-690 — ROT32L2R_imm8 / ROT64L2R_imm8 (`32-shamt`, `64-shamt`), RORX_Pats and ShiftX_Pats use of shiftMask32/64.
- C++ helpers re-derived: getVPCMPImmForCond / getSwappedVPCMPImm (X86InstrInfo.cpp:3485-3533); getSwappedVCMPImm (3563-3580); isUnneededShiftMask (X86ISelDAGToDAG.cpp:520-529); selectMOV64Imm32 (3071-3103); getExtractVEXTRACTImmediate / getInsertVINSERTImmediate / getPermuteVINSERTCommutedImmediate (454-480).
- Patterns ruled out:
  - BTRXForm uses countr_one(Imm) which correctly returns the index of the lowest 0 bit; matches BTRMask64's "single zero bit not in low 32" gate.
  - BTCBTSXForm uses countr_zero(Imm) (single 1 bit at pos ≥31 by BTCBTSMask64) — correctly accepts bit 31 (where OR64ri32 would sign-extend wrong) and rejects bit 30.
  - getPermuteVINSERTCommutedImmediate's `0x30 / 0x02` for vinsert(0/1, sub, vec) is correct: with src1 = `INSERT_SUBREG(undef, sub, sub_xmm)` and src2 = vec, byte 0x30 = (3<<4)|0 selects src1.lo (sub) + src2.hi (vec.hi); byte 0x02 = (0<<4)|2 selects src2.lo (vec.lo) + src1.lo (sub).
  - isUnneededShiftMask: `Mask = Val | KnownZeros; Mask.countr_one() >= Width` correctly captures "low Width bits of effective mask are 1" so the shift's hardware mask makes the AND redundant.
  - ROT32L2R_imm8 / ROT64L2R_imm8: `BW - shamt` is the right rorx imm; shamt=0 produces imm BW (0x20 or 0x40), but DAGCombiner canonicalizes rotate-by-0 to identity well before isel.
  - PrefetchWLevel: `<= 3` correctly bounds the IR locality argument.
  - immff00_ffff (`0xff00..0xffff`): `(X & mask) >> 8` reduces to high byte of low word for every mask in the range (low 8 bits ignored after the SRL).
  - selectMOV64Imm32 X86Wrapper-only gate is correct (WrapperRIP would be wrong for `mov $imm32, %rNd`).
- Potential bugs filed:
  - candidates/w38-gisel-loadi16-loadi32-ignores-EnablePromoteAnyextLoad.md — GISelPredicateCode for loadi16/loadi32 doesn't gate the anyext-widening on the `-x86-promote-anyext-load` cl::opt that the DAG predicate respects; flag silently ignored under `-global-isel`.
  - candidates/w38-extloadi64i32-ignores-EnablePromoteAnyextLoad.md — DAG path's `extloadi64i32` widens i8/i16 EXTLOAD-to-i64 to a 32-bit machine load unconditionally; missing the `EnablePromoteAnyextLoad` gate that sibling loadi16/loadi32 have.

## worker-50 2026-05-21
- File: llvm/lib/CodeGen/SelectionDAG/LegalizeDAG.cpp:1-6353 — full read; focused on `LegalizeLoadOps`/`LegalizeStoreOps` Promote+Custom paths, `ExpandFPLibCall`/`ExpandFastFPLibCall`/`canUseFastMathLibcall`, `ConvertNodeToLibcall` opcode table 4736-5343, `STRICT_FP_ROUND`/`STRICT_FP_EXTEND` mutate-to-FP path 3438-3492, FSUB->FADD+FNEG expansion 4055-4065, FP_TO_FP16 afn-flag fast path 4024-4039, `ExpandStrictFPOp` 4660-4710.
- File: llvm/lib/CodeGen/SelectionDAG/SelectionDAGISel.cpp:1-4650 — full read; focused on strict-FP mutate gate 1362-1391, `IsLegalToFold`/`findNonImmUse` glue-walk 2440-2508, `HandleMergeInputChains` quick-exit 2843-2914, `Select_INLINEASM` glue+chain 2510-2521, `MorphNode` chain/glue shift 2916-2960, `OPC_EmitMergeInputChains*` 4144-4222, `CaptureGlueInput` 3625-3629.
- Patterns ruled out:
  - LegalizeDAG `LegalizeLoadOps` Promote path (708-723) bitcasts result and forwards chain via `Res.getValue(1)`; no dropped chain.
  - LegalizeDAG FSUB->FADD+FNEG (4055-4065) propagates `Flags` only to the FADD; FNEG without flags is the safe default and the rewrite preserves IEEE result (`x + -y == x - y` in finite-math semantics; `nsz` not required because `FNEG(y)` toggles the sign bit deterministically and FADD then collapses signed-zero corner via standard IEEE rules — verified by case analysis on x=±0,y=±0).
  - LegalizeDAG `STRICT_FP_ROUND`/`STRICT_FP_EXTEND` mutate-to-FP at 3438/3472: gated on both `!isStrictFPEnabled()` AND the non-strict op being Legal — safe; chain is re-linked via `EmitStackConvert` on the stack path.
  - LegalizeDAG `ExpandFPLibCall` strict path (2226-2237): faithfully passes the chain through `makeLibCall` so chain ordering is preserved at the libcall boundary.
  - SelectionDAGISel `mutateStrictFPToFP` (SelectionDAG.cpp:12397-12436): re-links InputChain to OutputChain consumers BEFORE MorphNodeTo, preserving cross-BB chain order.
  - SelectionDAGISel `HandleMergeInputChains` size==1 quick-exit (2859-2860): single-matched-chain case has no TokenFactor to interpose between glue producer/consumer, so the InputGlue==V.getNode() guard at 2898 isn't structurally needed.
  - SelectionDAGISel `IsLegalToFold` glue walk (2492-2505): correctly forces `IgnoreChains=false` once we've walked through a glue edge, so a glue-bound user that also has a chain dependency on a sibling load is not silently folded.
- Potential bugs filed:
  - candidates/w50-strict-fp-routed-to-fast-libcall.md — `canUseFastMathLibcall` (LegalizeDAG.cpp:4727-4734) inspects only FMF and never checks `Node->isStrictFPOpcode()`. The `ConvertNodeToLibcall` cases for STRICT_FADD/STRICT_FSUB/STRICT_FMUL/STRICT_FDIV/STRICT_FSQRT (4855-5343) all call `ExpandFastFPLibCall(Node, canUseFastMathLibcall(Node), ...)`. A strictfp call to a constrained intrinsic that carries `afn nnan ninf nsz` (legal IR; SelectionDAGBuilder copies FMF onto the STRICT SDNode) selects `RTLIB::FAST_*`, which is by design an approximation that doesn't honor `fpexcept.strict` / non-default rounding. Latent on x86 (no FAST_* impl registered), live on any target that registers fast libcall impls (Hexagon today, others tomorrow). Structural source-level defect; fix is `if (Node->isStrictFPOpcode()) return false;` at the top of `canUseFastMathLibcall`.

## worker-42 2026-05-21
Scope: SimplifyCFG.cpp, LoopRotation.cpp, LoopUnrollPass.cpp.
- File: llvm/lib/Transforms/Utils/SimplifyCFG.cpp (9006 lines)
  - speculativelyExecuteBB (3204-3450) — guards via isSafeToSpeculativelyExecute
    for non-load/store hoists; load/store path requires isSafeCheapLoadStore
    which rejects volatile/atomic (line 1832-1837). Throws/atomics not
    speculated. OK.
  - hoistCommonCodeFromSuccessors / isSafeToHoistInstr (1536-1561,1848-2043) —
    skippedInstrFlags sets SkipImplicitControlFlow for `!isGuaranteedToTransfer`
    (covers mayThrow); isSafeToHoistInstr re-checks isSafeToSpeculativelyExecute
    under that flag. OK.
  - performBranchToCommonDestFolding (3949-4060) — addPredecessorToBlock copies
    PHI incoming-from-BB to new pred edge; cloneInstructionsIntoPredecessorBlock
    (1161-1238) rewrites block-closed-SSA PHI uses of bonus instructions in
    UniqueSucc. Walked all four (Or/And × Invert/no-invert) cases; UniqueSucc
    computation at line 3977 after InvertBranch is correct for all.
  - SwitchToLookupTable replaceSwitch LookupTableKind (6968-6993) — emits
    `Builder.CreateLoad(..., "switch.load")` with NO metadata. Constant table
    is private/unnamed_addr/constant, GEP is inbounds in a fully-initialized
    array — !dereferenceable, !invariant.load, and (when applicable) !nonnull
    / !range are all derivable but never attached. Missed optimization, not a
    miscompile. Filed.
  - hoistConditionalLoadsStores (1734-1846) — comment at 1808 claims `!nonnull`
    and `!align` aren't kept because pointer-typed loads aren't supported, but
    scalar pointer loads are reachable. The drop is nonetheless sound (false-mask
    passthrough is poison/zero). Filed as documentation candidate.
- File: llvm/lib/Transforms/Scalar/LoopRotation.cpp (107 lines) — pure pass-
  manager driver. No latch-PHI / rotation logic here. The named bug pattern
  must be looked for in `Utils/LoopRotationUtils.cpp::LoopRotate::rotateLoop`.
- File: llvm/lib/Transforms/Scalar/LoopUnrollPass.cpp (1877 lines) — cost /
  heuristics / pipeline glue. Calls `UnrollLoop` from `Utils/LoopUnroll.cpp`.
  No clone / metadata-copy / per-iteration code lives here.
- Patterns ruled out:
  - SpeculativelyExecuteBB hoisting mayThrow/atomic — guarded by
    isSafeToSpeculativelyExecute + isSafeCheapLoadStore.
  - FoldBranchToCommonDest PHI rewrite — block-closed-SSA loop catches all
    UniqueSucc PHI uses of bonus instructions.
  - SwitchToLookupTable dropping nonnull on pointer PHI — the new switch.load
    is a fresh load; dropping is sound but conservative.
- Potential bugs filed (all source-level, none ascend to miscompile):
  - candidates/w42-simplifycfg-switch-lookup-load-misses-deref-nonnull.md
  - candidates/w42-simplifycfg-hoistcondloads-drops-pointer-metadata.md
  - candidates/w42-loop-rotation-loop-unroll-driver-no-bugs.md (scoping note)

## worker-41 — VPlan vectorization (VPlan.cpp, VPlanRecipes.cpp, VPlanTransforms.cpp)
- File: llvm/lib/Transforms/Vectorize/VPlanRecipes.cpp — clone() & FMF handling
  - All `clone()` methods on VPRecipeBase subclasses (VPInstruction:1375,
    VPWidenRecipe:1782, VPWidenCastRecipe:1841, VPWidenIntrinsicRecipe:1922,
    VPWidenCallRecipe:1998, VPHistogramRecipe:2050 (no flags), VPWidenGEPRecipe:2106,
    VPVectorEndPointerRecipe:2197, VPVectorPointerRecipe:2250, VPBlendRecipe:2786,
    VPReductionRecipe:3071, VPInterleaveRecipe:2951, VPWidenLoadRecipe:3563,
    VPWidenStoreRecipe:3660, VPExpressionRecipe:3391) faithfully forward `*this`
    (Flags/Metadata) into the new recipe. *EVL recipes & VPExpandSCEVRecipe trivial.
  - VPWidenMemoryRecipe ctor derives Alignment from `getLoadStoreAlignment(&I)`
    on the underlying IR Load/Store; clone re-runs the same ctor, so alignment
    is preserved (never mutated post-construction). NOT a bug.
  - VPReductionRecipe::execute (2888-2946) and VPReductionEVLRecipe::execute
    (2948-2979): both push FMF via IRBuilder FMFGuard; ordered branch reads
    `RdxParts[N-1]` which is correct for unrolled ordered chains.
  - VPInstruction ComputeReductionResult (751-800): IsOrdered path takes the
    last partial and skips the cross-part reduce; IsInLoop guard prevents
    accidentally adding a tree reduction in-loop. OK.
- File: llvm/lib/Transforms/Vectorize/VPlanTransforms.cpp
  - simplifyRecipe (1332-1700): all FP-aware folds checked.
    - trunc(zext/sext) collapse (1362-1399) creates fresh widen-casts via
      createWidenCast (default flags) — drops `nneg/nsw/nuw` on the cast.
      Strictly weakening, not a miscompile.
    - "any-of (fcmp uno X,X), (fcmp uno Y,Y) -> any-of (fcmp uno X,Y)" fold
      (1567-1593) and the binary-or variant (1599-1606): semantically
      equivalent NaN-or checks. OK.
    - select !c,x,y -> select c,y,x (1479-1485) operates in-place; flags
      preserved. OK.
    - (X && Y)|(X && !Y) -> X (1406-1409) is an integer (i1) identity. OK.
    - createPartialReduction (6053-6063): integer Sub negation; no FMF needed.
    - createPartialReduction (6085-6090): FAdd partial reduction pulls FMF
      from the source WidenRecipe FAdd. OK.
    - tryToComputeEndValueForInduction FP arm (1131-1138): inherits FMF from
      the original FP induction binop. OK.
    - createFpInductionResumeValue (3861-3872): inherits FMF from FPBinOp. OK.
    - sinkRecurrencesAndFoldExitConds FindLastSelect path (5816): uses
      `FastMathFlags()` but only for integer Min/Max RecurKind; FP unused.
  - clearReductionWrapFlags (2351): only drops wrap; nothing FP-relevant.
  - cse (2466-2490): VPCSEDenseMapInfo::isEqual (2428) does NOT consider FMF
    in equality, then calls `intersectFlags`. See finding below.
- Patterns ruled out:
  - VPWidenMemoryInstructionRecipe alignment-drop on clone — alignment is
    sourced from the underlying load/store and never re-set; clones preserve.
  - VPInstruction::ComputeReductionResult ordered-path incorrect-cross-part
    reduce — correctly skipped for IsOrdered.
  - VPRecipeBase clone dropping FMF — every clone forwards `*this` as Flags.
- Potential bugs filed:
  - candidates/w41-vplan-cse-intersectflags-fmf-wrong-direction.md —
    `VPIRFlags::intersectFlags` (VPlanRecipes.cpp:343-391) for `FPMathOp` /
    `FCmp` / `ReductionOp` arms only touches NoNaNs and NoInfs (anding them),
    leaving AllowReassoc / AllowReciprocal / AllowContract / ApproxFunc /
    NoSignedZeros untouched. LLVM's canonical merge in `FMF.h:118-130`
    (`intersectRewrite` ANDs the rewrite-permission bits; `unionValue` ORs the
    value bits NoNaNs/NoInfs/NoSignedZeros). VPlan-CSE (VPlanTransforms.cpp:2466)
    can therefore retain reassoc/contract permissions from V that V's original
    author granted but Def's downstream chain forbade — those users now reference
    V and observe the permissions. Anding NoNaNs/NoInfs is the wrong direction
    (conservatively safe, just loses information). Static-analysis only;
    structural root cause matches the documented FMF.h merge contract.

## worker-49 2026-05-21
- File: llvm/lib/Target/X86/X86OptimizeLEAs.cpp:1-784 — full read; MemOpKey/isIdenticalOp/isSimilarDispOp, chooseBestLEA dist+disp tradeoff, removeRedundantAddrCalc DefMI-lift safety argument (relies on SSA), removeRedundantLEAs iterator/list mutation, debug-value replacement.
- File: llvm/lib/Target/X86/X86LowerTileCopy.cpp:1-174 — full read; UsedRegs.stepBackward+available scan, GR64Cand fallback to RAX with spill/reload, GET_EGPR_IF_ENABLED TILELOADD/TILESTORED rewrite, missing MachineMemOperand on spill/reload MOVs.
- File: llvm/lib/Target/X86/X86LoadValueInjectionLoadHardening.cpp:1-845 — full read; getGadgetGraph (AnalyzeDefUseChain Dead/self-ref checks at lines 419-422, TraverseCFG revisit early-return at 501-502), elimMitigatedEdgesAndNodes DFS, hardenLoadsWithHeuristic ingress/egress cut cost, insertFences branch-arm CutEdges mutation, instrUsesRegToAccessMemory/Branch.
- File: llvm/lib/Target/X86/X86CleanupLocalDynamicTLS.cpp:1-165 — full read; ReplaceTLSBaseAddrCall vs SetRegister iterator dance after I.eraseFromParent / BuildMI-insert. Iterator semantics correct.
- File: llvm/lib/Target/X86/X86GlobalBaseReg.cpp:1-145 — full read; PIC large-code-model PBReg/GOTReg/ADD sequence, RIP-relative LEA path, MovePCtoStack + ADD32ri for 32-bit GOT-style. No issues spotted.
- File: llvm/lib/Target/X86/X86ArgumentStackSlotRebase.cpp:1-205 — full read; getArgBaseReg (C / X86_RegCall on 64-bit only), IsBaseRegisterClobbered inline-asm scan, PLEA64r/PLEA32r setup, eliminateFrameIndex call inside operand iterator.
- File: llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp:115-2200, 3253-3540 (Part 3 areas requested) — focused on simplifyX86immShift (per-imm vs per-scalar, DemandedLower/Upper for v4i32/v8i16/v2i64), simplifyX86varShift AnyOutOfRange single-LogicalShift type invariant, simplifyX86pack signed/unsigned saturation algebra, simplifyX86pmulh PMULH/PMULHU/PMULHRSW (vXi18 rounding LShr-trunc-add-LShr trick — i18 trunc absorbs unsigned-shift garbage), simplifyX86pmadd, simplifyX86pshufb (Index = ((Idx<0)?NumElts:Idx&0x0F)+(I&0xF0) per-lane semantics), simplifyX86vpermilvar (PS bits[1:0], PD bit 1 via getLoBits(2)>>1), simplifyX86vpermv/v3 (mask &= Size-1 / 2Size-1), simplifyTernarylogic (uint8_t pair-second imm-tracking; no end-of-switch assertion that Res.second == Imm), simplifyDemandedVectorEltsIntrinsic (PSHUFB/VPERMILVAR/PERMV only demand op1 mask).
- Patterns ruled out:
  - X86OptimizeLEAs MemOpKey hash-vs-eq: hash explicitly excludes Disp's imm value while eq tolerates imm diff; key collisions are intentional and resolved by isSimilarDispOp; lift safety holds under SSA invariant (asserted via MachineFunctionProperties::IsSSA at pipeline entry).
  - X86LowerTileCopy LiveRegUnits.addLiveOuts + stepBackward correctly tracks "live above MI" at the COPY's position; RAX-spill fallback is semantically transparent.
  - X86CleanupLocalDynamicTLS iterator dance: ReplaceTLSBaseAddrCall returns the COPY inserted before I (which then becomes the new I), `++I` correctly advances past it; SetRegister inserts COPY after I (original is preserved as the compute), `++I` advances past the COPY. Both cases are correct.
  - X86GlobalBaseReg large-CM LEA + MOV64ri + ADD64rr ordering and getPICBaseSymbol attachment via setPreInstrSymbol on std::prev(MBBI) is correct.
  - X86InstCombineIntrinsic simplifyX86immShift `DemandedUpper = getBitsSet(NumAmtElts, 1, NumAmtElts/2)` is exactly right for v4i32 (bit 1), v8i16 (bits 1..3), and v2i64 (empty -> skip via `DemandedUpper.isZero()`).
  - simplifyX86vpermilvar PD path bit-1 extraction via `zextOrTrunc(32).getLoBits(2).lshrInPlace(1)` matches Intel hardware mask[63:0] bit 1 semantics (bit 1 of i64 = bit 1 of i32 truncate).
  - simplifyX86pmulh PMULHRSW signed/unsigned LShr by 14 then trunc-to-i18 trick: trunc to 18 bits discards the upper 14 bits where LShr and AShr disagree, so subsequent +1, LShr 1, trunc i16 yields the correct bits[16:1].
  - simplifyX86varShift `AnyOutOfRange = LogicalShift` per-iter assignment is safe because each call handles a single fixed shift type; mixed in-range/out-of-range bails at line 411 only for LogicalShift.
- Potential bugs filed:
  - candidates/w49-lvi-analyzedefusechain-dead-check-wrong-def.md — `Def.Addr->getAttrs() & NodeAttrs::Dead` checks parent Def (loop-invariant) instead of ChildDef; self-reference `Def.Id == ChildDef.Id` is structurally always-false and probably meant SourceDef.Id (perf+analysis correctness)
  - candidates/w49-lvi-cfg-traverse-skips-instrs-on-revisit.md — TraverseCFG early-returns after recording the entry edge on revisit, skipping the successor-recursion loop — under-approximates reachability, security-correctness concern
  - candidates/w49-lvi-insertfences-branch-mutates-during-iteration.md — branch-arm inserts all CFG egress into CutEdges while outer loop iterates the same graph edges; can over-cut for plugin's per-iteration trim accounting (latent)
  - candidates/w49-optimize-leas-choose-best-after-MI.md — lift-LEA-above-MI safety argument depends on SSA invariant; pass lacks an `MachineFunctionProperties::IsSSA` assertion (maintenance landmine, not a current miscompile)
  - candidates/w49-lower-tile-copy-no-rax-spill-check.md — tile-copy spill/reload MOV64mr/MOV64rm lack MachineMemOperand; post-RA scheduler could reorder around them and break the "RAX above = RAX below" protocol
  - candidates/w49-arg-stack-slot-iterator-invalidated-by-eliminateFI.md — operand range-for holds MachineOperand& across TRI->eliminateFrameIndex which has broad-rewrite permission (latent landmine; current x86 in-place rewrite is safe)
  - candidates/w49-cleanup-tls-iterator-invalidated.md — ruled-out note (iterator dance correct after re-read)
  - candidates/w49-instcombine-vpermilvar-pd-mask-truncates-bit-1.md — ruled-out note (zext-trunc-shift sequence matches Intel hardware)
  - candidates/w49-instcombine-imm-shift-upper-demand-wrong-for-i32.md — ruled-out note (DemandedUpper mask is exactly right for all NumAmtElts)

## worker-39 2026-05-21
- File: llvm/lib/Target/X86/MCTargetDesc/X86EncodingOptimization.cpp:1-504 — full read; focused on optimizeInstFromVEX3ToVEX2 (default commutable swap + `_REV` rewrite), optimizeShiftRotateWithImmediateOne, optimizeVPCMPWithImmediateOneOrSix, optimizeMOVSX/INCDEC/MOV, optimizeToShortImmediateForm/optimizeToFixedRegisterForm, X86EncodingOptimizationForImmediate.def table.
- File: llvm/lib/Target/X86/MCTargetDesc/X86MCCodeEmitter.cpp:1-1600 — X86OpcodePrefixHelper (REX/REX2/VEX2/VEX3/XOP/EVEX bit math, set R/X/B/R2/X2/B2/V2/4V/4VV2 and U/b/z/L/L2/aaa/NF/SC), determineOptimalKind, emit(), emitMemModRMByte (RIP-rel, 16-bit, no-SIB and SIB paths, [disp32] in 64-bit via needSIB, disp8/cdisp8), emitREXPrefix (high-byte conflict guard, S_GOTTPOFF/TLSDESC x32), emitVEXOpcodePrefix per-form (MRMSrcReg/Mem, MRMDestReg/Mem, MRMXrCC, etc., HasTwoConditionalOps SC).
- File: llvm/lib/Target/X86/MCTargetDesc/X86AsmBackend.cpp:1-980 — full read; focused on isRelaxableBranch/getRelaxedOpcodeBranch (JCC_1/JMP_1 → _2 in 16-bit, _4 otherwise), mayNeedRelaxation (CCMP SkipOperands=2), fixupNeedsRelaxationAdvanced (S_ABS8 special), applyFixup overflow check (signed/unsigned mask logic), evaluateFixup PCRel adjustment + _GLOBAL_OFFSET_TABLE_ → reloc_global_offset_table, determinePaddingPrefix (RawFrmDstSrc/Src/MemOffs segment extraction, 64-bit CS default, 32-bit ESP/EBP→SS, else DS), padInstructionViaRelaxation/ViaPrefix, finishLayout.
- Patterns ruled out:
  - REX/REX2 EGPR bit routing — setRR2/setBB2/setXX2 conditionally set R2/B2/X2 only when Kind<=REX2 or operand is APX EGPR (vector regs that have bit 4 don't trigger REX2-only bits).
  - EVEX byte layout matches Intel APX spec: P0[7:4]=~R/~X/~B/~R4 (inverted), P0[3]=B4 (NOT inverted), P1[2]=~X4 (inverted), P2[3]=~V4 (inverted). All bit positions and inversion polarities check out.
  - applyFixup signedness check: the `(Value & Mask) && (Signed ? != Mask : (-Value & Mask))` formula correctly accepts the documented "unknown-signedness" range (-2^N, 2^N) for unsigned and [-2^(N-1), 2^(N-1)) for signed.
  - `mayNeedRelaxation` for CCMP uses isCCMPCC(Opcode) to skip 2 trailing operands (cond, dcf); the Operands[size-1-Skip].isExpr() index then correctly addresses the imm slot for both CCMP*ri and CCMP*mi.
  - CTEST not in EncodingOptimizationForImmediate.def is correct: TEST/CTEST opcode F7 has no sign-extended imm8 short form (group 3, unlike CMP/ADD group 1 with 0x83).
  - needSIB(In64BitMode) forces SIB when BaseReg is 0, so the non-SIB `if (!BaseReg)` path emitting `modRM(0,reg,5)` (= disp32 in 32-bit, RIP-rel in 64-bit) only ever runs in 32-bit mode where the encoding genuinely means absolute disp32.
  - optimizeMOV IsStore variable is misleadingly named (it's actually IsLoad), but the resulting AddrBase/RegOp indices are correct because op1 is Scale (imm) in stores vs BaseReg (reg) in loads.
  - determinePaddingPrefix correctly extracts segment from operand 1/2 for RawFrmSrc/RawFrmDstSrc/RawFrmMemOffs (matches X86SrcIdxOperand layout `(ptr, segment)`).
- Potential bugs filed:
  - candidates/w39-vex3-to-vex2-xmm16-31-not-rejected.md — optimizeInstFromVEX3ToVEX2 uses isX86_64ExtendedReg which conflates VEX-encodable extended regs (XMM8-15, R8-R15) with EVEX-only regs (XMM/YMM/ZMM16-31, R16-R31). The swap can therefore place an EVEX-only register into a VEX field, silently losing the high bit. Static-only; not believed to be reachable from ISel-emitted code today but is structurally wrong and is reachable from the AsmParser entry point.

## worker-37 2026-05-21
- File: llvm/lib/Target/X86/X86InstrAVX512.td — spot-read predicate blocks
  around: NoVLX 256/128 widen patterns (lines 4459-4618, 5095-5162, 6143-6293,
  9949-10002, 11944-12033), AVX512 vpcmp/vcmpm pattern blocks (2100-2473),
  Vfpclass scalar/vec (2480-2570), VPTERNLOG vnot patterns, mtrunc_lowering.
- File: llvm/lib/Target/X86/X86InstrSSE.td — spot-read: MOVLP/MOVHP patterns
  (700-826), cvtsi2ss/sd MOV[SS|SD] Pat<> blocks (1420-1530), Blend patterns
  (6230-6320), broadcast Pat<> blocks (7100-7820), VINSERTPS (5460-5488).
- Patterns ruled out (after structural check):
  - VPTERNLOG vnot rewrite with imm 15 (11944-12033) — correct: imm 15 with
    all three operands = src gives ~src (the four set bits cover (a,b,c) with
    a=0, regardless of b/c).
  - avx512_rotate_novlx widening (6265-6293) — imm and variable rotates fold
    correctly: AVX-512 V[P]ROL[V]Q uses only low 6 bits of count, upper
    IMPLICIT_DEF lanes are extracted away by sub_xmm/sub_ymm.
  - avx512_binary_lowering NoVLX widen (4900-4956) — INSERT_SUBREG sub_xmm /
    sub_ymm + EXTRACT_SUBREG round-trip preserves only the requested lanes.
  - MOVLP / MOVHP SSE vs AVX (810-820 SSE2 X86Shufp vs 782-784 AVX X86VPermilpi
    with imm 1) — both extract the same v2f64 lane (element 1 → low).
  - SSE2 missing cvtsi2ss patterns (1481-1513) — not a gap, UseSSE1 patterns
    at 1515+ also fire under SSE2 (UseSSE1 = hasSSE1 && !hasAVX).
  - VPBROADCASTW for f16 under [HasAVX2, NoVLX_Or_NoBWI] (7786-7794) — bit-
    equivalent broadcast, FR16X is an alias of FR32X (always present).
- Potential bugs filed:
  - candidates/w37-avx512-vmovs-x86selects-load-fold-mask-suppress.md —
    Pat<> rules at X86InstrAVX512.td:4321-4327 / 4339-4345 fold an
    unconditional `loadf32` / `loadf64` into the masked-load form
    `VMOVSSZrmk` / `VMOVSDZrmk` (and the *kz no-passthru variants). The
    PatFrag uses vanilla `load` (no `isSimple` / `dereferenceable_load`
    guard), so when `X86selects` was created from an `ISD::SELECT` with a
    fault-prone load in the true arm, the mask=0 case silently suppresses
    a load that the source IR was required to execute. Latent — needs
    actually-unmapped memory in the false-mask path to be observable, and
    earlier load-hoisting often masks it.

## worker-43 2026-05-21
- File: llvm/lib/CodeGen/ImplicitNullChecks.cpp:1-812 — full read; focused on canHandle/canReorder/computeDependence, isSuitableMemoryOp (BaseReg/ScaledReg + Displacement w/ APInt overflow check), areMemoryOpsAliased (MMO PSV vs Value), canDependenceHoistingClobberLiveIns (NullSucc live-in scan), canHoistInst, analyzeBlockForNullChecks (MBP / make_implicit metadata gate, PointerReg modification scan), insertFaultingInstr (build of FAULTING_OP), rewriteNullChecks (live-in propagation).
- File: llvm/lib/CodeGen/MachineCopyPropagation.cpp:1-1643 — full read; focused on CopyTracker (regunit-keyed, DefRegs tracking, clobberRegUnit's DefRegs cleanup, findAvailCopy + findAvailBackwardCopy regmask scans, findCopyDefViaUnit, getPreservedRegUnits memoization), eraseIfRedundant (isNopCopy w/ subreg index), forwardUses (hasImplicitOverlap gate, isForwardableRegClassCopy cross-class search, sub-register forwarding math), propagateDefs / canUpdateSrcUsers / hasOverlappingMultipleDef, backwardCopyPropagateBlock (implicit-operand gate at 1183), spillage-chain elimination.
- File: llvm/lib/CodeGen/TwoAddressInstructionPass.cpp:1-2158 — full read; focused on collectTiedOperands (undef rewrite shortcut), processTiedPairs (RegA==RegB skip, COPY insertion + LIS/LV updates, RemovedKillFlag bookkeeping, AllUsesCopied path), tryInstructionTransform (commute → conv3addr → reschedule → unfold-load ladder), convertInstTo3Addr → TII->convertToThreeAddress, scanUses/processCopy SrcRegMap/DstRegMap maintenance, processStatepoint tied-pair rewrite, rescheduleMIBelowKill/AboveMI hazard scans.
- File: llvm/lib/Target/X86/X86InstrInfo.cpp:1405-1660 (X86 convertToThreeAddress + hasLiveCondCodeDef + classifyLEAReg gates), :4391-4403 (X86 isCopyInstrImpl — undef-subreg dirty-hack carve-out), :855-889 X86MCInstLower::LowerFAULTING_OP — verified implicit operands are dropped at lowering via LowerMachineOperand `if (MO.isImplicit()) return MCOperand();`.
- Patterns ruled out:
  - INC `canHandle` correctly rejects calls, mayRaiseFPException, hasUnmodeledSideEffects, ordered atomics (MMO->isUnordered).
  - INC `isSuitableMemoryOp` APInt smul_ov / sadd_ov overflow protection on `CalculateDisplacementFromAddrMode` is sound.
  - INC `insertFaultingInstr` MMO list preservation via `setMemRefs` — MMO flags (volatile/atomic/dereferenceable) survive.
  - MCP backward propagation's `MI.getNumImplicitOperands() == 0` gate at line 1183 — implicit-operand-bearing copies correctly excluded.
  - MCP spillage-chain `GetFoldableCopy` gate at line 1399 — same correct guard.
  - MCP CopyTracker `clobberRegUnit` -> `markRegsUnavailable(DefRegs)` -> per-unit `Avail=false` correctly invalidates a forward-tracked copy when its source is clobbered through ANY sub-register unit.
  - MCP forward-propagation `findAvailCopy` regmask scan (lines 397-402) is redundant with tracker's per-MI regmask invalidation but not contradictory.
  - 2addr `convertToThreeAddress` correctly gates EFLAGS-live conversion via `hasLiveCondCodeDef`; LEA encoding doesn't clobber EFLAGS so flags-reuse miscompile not possible on that path.
  - 2addr `processTiedPairs` COPY insertion is `TargetOpcode::COPY` (no EFLAGS clobber); lowered by `copyPhysReg` to MOV*rr which also preserves EFLAGS.
  - 2addr `tryInstructionTransform` unfold-load path correctly threads kill/dead flags via LV->replace*/add* and rebuilds LIS via `RemoveMachineInstrFromMaps + InsertMachineInstrInMaps`.
  - X86 isCopyInstrImpl correctly bails on undef+subreg COPYs.
- Potential bugs filed:
  - candidates/w43-mcp-eraseIfRedundant-drops-implicit-operands.md — forwardCopyPropagateBlock's eraseIfRedundant lacks the `getNumImplicitOperands() == 0` gate that backward/spillage paths use; can erase a COPY that carries unique implicit-def operands and leave the destination of those implicit defs undefined.
  - candidates/w43-mcp-hasImplicitOverlap-misses-implicit-def-of-source.md — forwardUses checks only implicit-USES for overlap with the rewritten operand, and gates the "MI modifies CopySrc" bail on `isCopyInstr(MI)`; non-copy users with implicit-DEFs of CopySrc bypass both checks. Filed as structural fragility (no confirmed miscompile on x86 today).
  - candidates/w43-implicitnullchecks-insertFaultingInstr-loses-mi-flags.md — `insertFaultingInstr` constructs the FAULTING_OP with empty DebugLoc, drops `MI->getFlags()` (incl. MIFlag::Unpredictable), and rebuilds the def operand from bare `getReg()` losing `isRenamable`/`isEarlyClobber`/subreg index from the original load's def.

## worker-51 2026-05-21
- File: llvm/lib/CodeGen/GlobalISel/LegalizerInfo.cpp:1-464 — full read. Framework-only file (rule matching, mutation sanity, alias / coverage verify). No target-specific (x86) rules here; X86LegalizerInfo.cpp itself was covered by w14.
- File: llvm/lib/CodeGen/GlobalISel/MachineIRBuilder.cpp:1-1581 — full read. Focus on dbg-value emission, ptr-add / ptr-mask / mask-low-ptr-bits, atomic-rmw family, intrinsic flag derivation, validateUnary/Binary/Shift/Select/TruncExt, the BUILD_VECTOR / CONCAT_VECTORS / UNMERGE / MERGE / SUBVECTOR validation switch (1278-1581).
- File: llvm/lib/CodeGen/GlobalISel/CallLowering.cpp:1-1429 — full read. Focus on addFlagsFromAttrSet attribute table, setArgFlags byval/byref alignment, splitToValueTypes register-block tracking, buildCopyFromRegs / buildCopyToRegs (scalar-and-vector shape coercion), determineAssignments split-arg flag bookkeeping, handleAssignments byval/sret/indirect paths, insertSRetIncoming/Outgoing demote-arg construction, checkReturn / getReturnInfo, resultsCompatible tail-call CC check.
- File: llvm/lib/CodeGen/GlobalISel/Utils.cpp:1-2080 — full read. Focus on constrainOperandRegClass / constrainSelectedInstRegOperands, getIConstantVRegVal + look-through walker (with ANYEXT->SEXT collapse), ConstantFoldBinOp / ICmp / ExtOp / Unary, getLCMType / getCoverTy / getGCDType, canCreateUndefOrPoison / isGuaranteedNotToBeUndefOrPoison, shiftAmountKnownInRange, GIConstant / GFConstant accessors.
- Patterns ruled out:
  - LegalizerInfo.cpp `mutationIsSane` (118-189) — NarrowScalar/WidenScalar invariants, FewerElements/MoreElements element-count strictness, Bitcast size-equality all correctly enforced.
  - MachineIRBuilder validate{Unary,Binary,Shift,Select,TruncExt} preconditions all symmetric across Res/Op0/Op1 (188-1275).
  - CallLowering.cpp `parametersInCSRMatch` (1170-1220) correctly walks COPY chains and rejects multi-reg args.
  - CallLowering.cpp `resultsCompatible` (1222-1274) — when CallerCC == CalleeCC short-circuits correctly; reg/mem location comparison is symmetric.
  - Utils.cpp `extractParts` (508-609) — leftover-vector unmerge math (`MainNumElts % LeftoverNumElts == 0` && `LeftoverNumElts > 1`) handles the irregular split case (e.g. v6i32 -> v4i32 + v2i32) without losing elements.
  - Utils.cpp `isKnownToBeAPowerOfTwo` (1071-1146) — G_AND with `OrNegative=false` correctly rejects (both halves only need to be pow2 with OrNegative=true).
  - Utils.cpp `ConstantFoldICmp` (993-1069) — APInt comparison opcodes routed via APInt sgt/slt/ugt/ult helpers; no signed/unsigned confusion.
  - MachineIRBuilder.cpp `buildAtomicCmpXchg{,WithSuccess}` (1024-1076) — type-mismatch asserts cover all four operand pairs; MMO required.
- Potential bugs filed:
  - candidates/w51-utils-canCreateUndefOrPoison-missing-div.md — `canCreateUndefOrPoison` switch omits G_SDIV / G_UDIV / G_SREM / G_UREM. The default arm returns `!isa<GBinOp>` which is false for divisions. Result: `isGuaranteedNotToBeUndefOrPoison` falsely reports a div result as guaranteed-defined, enabling speculative hoisting or freeze-removal that introduces UB-trap on a path the source never executed.
  - candidates/w51-calllowering-sret-demote-inherits-return-attrs.md — `insertSRetIncoming/OutgoingArgument` call `setArgFlags(DemoteArg, ReturnIndex, ...)` which copies SExt/ZExt/InReg/Returned flags from the IR return attribute onto the demote pointer ArgInfo. SelectionDAG path does NOT do this; for i686 regcall / Win64 with `signext` return, the demote pointer can be CC-assigned to a sign-extending location.
  - candidates/w51-mirbuilder-buildvector-vacuous-assert.md — G_BUILD_VECTOR / G_BUILD_VECTOR_TRUNC / G_CONCAT_VECTORS each use the dead assertion `(!SrcOps.empty() || SrcOps.size() < 2)` (always true) where `SrcOps.size() >= 2` was intended. 0/1-source variants slip through the validator.
  - candidates/w51-mirbuilder-buildMaskLowPtrBits-truncates-wide-ptr.md — `buildMaskLowPtrBits` builds the mask via `maskTrailingZeros<uint64_t>(NumBits)` and hands it to `buildConstant(int64_t)`. For PtrTy wider than 64 bits, the upper mask bits are zero, causing G_PTRMASK to clobber the high portion of the pointer. Latent for x86_64 (PtrSize==64).
  - candidates/w51-utils-lookthrough-anyext-treats-as-sext.md — `getConstantVRegValWithLookThrough` with `LookThroughAnyExt=true` reconstructs the APInt for a traversed G_ANYEXT via `Val.sext()`. Constant-fold callers and codegen lowering may disagree on the chosen extension, materializing a value the original IR never had.

## worker-44 2026-05-21
- File: llvm/lib/Analysis/ScalarEvolution.cpp:9430-9530, 13219-13707 — howManyLessThans + computeExitLimitFromICmp + computeMaxBECountForLT; isKnownNonZero (11255), isKnownNegative/Positive (11239-11252).
- File: llvm/lib/Analysis/ValueTracking.cpp:1439-1500 (UDiv/SDiv computeKnownBits), 4707-6330 (computeKnownFPClass dispatch incl. FNeg/Select/Load/fabs/copysign/fma/sqrt/sin/cos/tan/sinh/cosh/tanh/asin/acos/atan/powi/ldexp/amdgcn_rsq), 7252-7305 (UDiv/URem/SDiv/SRem isSafeToSpeculativelyExecute).
- File: llvm/lib/Support/KnownFPClass.cpp:554-912 (sqrt/sin/cos/tan/sinh/cosh/tanh/asin/acos/atan/powi propagation).
- File: llvm/lib/Support/KnownBits.cpp:1156-1296 (sdiv/udiv/urem/srem KnownBits poison-refined-to-zero comments).
- File: llvm/lib/Analysis/ConstantFolding.cpp:1192-1733 (canConstantFoldCallTo / ConstantFoldCall dispatch), 2195-2310 (GetConstantFoldFPValue, ConstantFoldFP, ConstantFoldBinaryFP with host-double + fenv), 2538-3175 (ConstantFoldScalarCall1 incl. type gating at 2639-2641, NVVM f2i/d2i, transcendentals routed to host libm), 3348-3417 (ConstantFoldLibCall2 pow/fmod/atan2/nextafter), 4080-4170 (ConstantFoldScalarCall3 fma/fmuladd path).
- Patterns ruled out:
  - SCEV computeMaxBECountForLT (13219-13266) correctly switches signed vs unsigned ranges and `StrideForMaxBECount = max(1, MinStride)` covers the stride==0 fallback.
  - SCEV canProveNUW (13280-13306) Limit calculation is self-limiting: large unsigned StrideMax shrinks Limit so RHS<=Limit gate effectively prevents wrong NUW inference even when isKnownNonZero admits unsigned-huge step.
  - SCEV computeExitLimitFromICmp self-non-wrap inference (9491-9512) via isKnownToBeAPowerOfTwo(Step, OrZero, OrNegative) is sound: under mustprogress+single-exit+loop-invariant-RHS, self-wrap of IV produces a repeating finite-period sequence that under mustprogress must be UB.
  - ValueTracking::isKnownNeverNaN/computeKnownFPClass for FNeg/fabs correctly preserve NaN class (KnownFPClass::fneg/fabs in KnownFPClass.h:185-217 don't flip fcNan/fcSNan/fcQNan bits; only sign-bit and ordered sign classes change).
  - KnownBits::udiv / urem "result is Zero or UB → return Zero" (KnownBits.cpp:1217-1222) is sound because the result IS poison when divisor==0 (poison refines to zero).
  - ConstantFolding::ConstantFoldFP gates non-half/float/double types at line 2639 ("Long double not supported yet"); f80/fp128 don't reach host-double libm (modulo `HAS_IEE754_FLOAT128 && HAS_LOGF128` carve-out at 2625-2637 which uses logf128 directly).
  - KnownFPClass::powi (KnownFPClass.cpp:843-912) over-approximates `powi(SNaN, 0)` result class to include fcSNan; conservative, not a soundness bug.
- Potential bugs filed:
  - candidates/w44-howmanylessthans-unsigned-uses-signed-rhsstride-check.md — howManyLessThans dual-AddRec branch (SCEV.cpp:13463-13505): for unsigned LT, gates the `ceil((End-Start)/(Stride-RHSStride))` formula via signed predicates (`isKnownNegative(RHSStride)` + `willNotOverflow(Sub, /*Signed=*/true, ...)`) and only requires `any(NoWrapFlags)` (NSW alone admissible) on the RHS recurrence — wrong polarity for the unsigned consumer.
  - candidates/w44-constantfoldfp-host-libm-variance.md — ConstantFolding host-libm routing (ConstantFolding.cpp:2263-2310, 3348-3417, callers at 2849-3175): `pow`, `sin`, `cos`, `log`, `exp`, `atan2`, etc. are evaluated through the *build host's* libm; differing libm last-ULP precision + boundary behavior (`pow(-1, ±inf)`, `pow(NaN, 0)`, `atan2(±0, ±x)`) bake host-dependent bits into the IR. Also half/float fold via `ConstantFoldFP` is doubly rounded (host-double then convert to target type).

## worker-52 2026-05-21
- File: llvm/lib/CodeGen/SelectionDAG/ScheduleDAGRRList.cpp:1-3201 — focused on EmitNode / ScheduleNodeBottomUp glue traversals (lines 581-597, 695-732, 736-818, 858-887), CopyAndMoveSuccessors (1130-1216, outgoing-glue-rejects, incoming-glue gate via canCopyGluedNodeDuringSchedule), InsertCopiesAndMoveSuccs (1218-1269), TryUnfoldSU dep classification (1054-1109), DelayForLiveRegsBottomUp (1348-1471), CheckForLiveRegDef / CheckForLiveRegDefMasked, FindCallSeqStart and CallSeqEnd tracking.
- File: llvm/lib/CodeGen/SelectionDAG/InstrEmitter.cpp:1-1484 — full read. Focus on EmitCopyFromReg phys-reg copy elision (86-183), CreateVirtualRegisters (185-263), AddRegisterOperand kill-flag inference (314-392), AddOperand reg-mask/global-addr/CP/block-addr dispatch (397-470), EmitSubregNode (510-655), EmitMachineNode (1001-1260, esp. HasPhysRegOuts implicit-def-as-CopyFromReg loop 1175-1184 and glued-user UsedRegs scan 1186-1210), EmitSpecialNode (1262-1472) INLINEASM/INLINEASM_BR (1343-1470 incl. early-clobber-also-input removal 1446-1453).
- File: llvm/lib/CodeGen/SelectionDAG/FastISel.cpp:1-2419 — full read. Focus on selectInstruction main loop + dead-code rollback (1543-1614), selectCall + inline-asm bare-constraint path (1151-1188), selectIntrinsicCall switch (1379-1441), selectStackmap (643-712), selectPatchpoint (753-898), selectXRayCustomEvent / selectXRayTypedEvent (900-938), fastEmit_* defaults (1885-1914), handleDbgInfo / lowerDbgValue / lowerDbgDeclare (1190-1390), selectOperator dispatch (1742-1866).
- Patterns ruled out:
  - Scheduler-reorders-past-glue: ScheduleDAGRRList correctly walks getGluedNode() in all live-region updates (LiveRegDefs, LiveRegGens, CallResource), in EmitNode opcode-dispatch (whole glue chain marked emitted), and in CopyAndMoveSuccessors clone-rejection. Outgoing-glue blocks clone path. canCopyGluedNodeDuringSchedule hook gates incoming-glue.
  - InstrEmitter implicit-def emission: EmitMachineNode's HasPhysRegOuts loop (1175-1184) correctly walks implicit_defs[i-NumDefs] and emits CopyFromReg for each one with a SDNode use, then setPhysRegsDeadExcept covers the un-used ones at 1219-1220. CreateVirtualRegisters' optional-def branch (220-225) is bypassed only when II.operands()[i].isOptionalDef() is false, otherwise vbase comes from RegisterSDNode operand — correct.
  - InstrEmitter glue-user UsedRegs scan (1186-1210): collects implicit_uses of downstream glued machine nodes + direct RegisterSDNode operands; CopyFromReg/CopyToReg correctly skipped. No INLINEASM-glued-after case observed in test corpus, but `F->getMachineOpcode()` would assert if hit (latent fragility, not filed — no crash repro from random IR yet).
  - FastISel selectInstruction rollback (1601-1605): removes dead local-value code on failure so SDAG fallback re-selects the failing instruction cleanly; no side-effect drop on the normal failure path.
  - FastISel selectCall inline-asm path (1155-1180): bails on non-empty constraint string before partial emission, so any side effects (MayLoad/MayStore from constrained operands) are picked up by SDAG fallback. ExtraInfo bits HasSideEffects/IsAlignStack/MayUnwind/IsConvergent/AsmDialect all correctly forwarded.
  - FastISel `Intrinsic::expect{,_with_probability}` / `launder/strip_invariant_group` (1410-1418) return getRegForValue of operand 0 — semantically correct (these intrinsics are identity transforms).
  - FastISel `Intrinsic::fake_use` (1420-1426) silently drops on getRegForValue failure — intentional (FAKE_USE is a debug hint, droppable).
- Potential bugs filed:
  - candidates/w52-fastisel-xray-customevent-dropped-on-aarch64.md — `selectXRayCustomEvent` / `selectXRayTypedEvent` guard `if (Triple.isAArch64(64) && Triple.getArch() != Triple::x86_64) return true;` is tautologically equivalent to "if AArch64-64, drop without emitting". On AArch64-64 with `-fast-isel=true`, `llvm.xray.customevent` / `llvm.xray.typedevent` (side-effect intrinsics with working PATCHABLE_EVENT_CALL lowering in AArch64 backend) emit no MI. This is the "FastISel fall-back path that drops a side-effect intrinsic" pattern. x86 is unaffected (predicate's first conjunct false), so the bug surfaces only on AArch64. Filed despite x86-focus because the offending file is on the hunt list and the pattern matches exactly.

## worker-48 2026-05-21
- File: llvm/lib/Transforms/InstCombine/InstCombineAddSub.cpp — focused on: nsw/nuw propagation in `(X|C2)+C` disjoint-or path (901-925), `~X+C → (C-1)-X` (894-903), `(1<<N)-1 → ~(-(1<<N))` canonicalizeLowbitMask (1233-1249), `-A+B → B-A` & `A+-B → A-B` (1646-1666), `(X<<S)+(Y<<S) → (X+Y)<<S` joint shift fold (1480-1507), `OptimizePointerDifference` mul nuw inference (2280-2317), sub-of-add simplifications (2643-2685).
- File: llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp — focused on: `(X<<C2)*C1 → X*(C1<<C2)` (232-246), `X*2^C → X<<C` nsw/nuw guard (247-263), `mul Op0 Op1 → shl` via tryGetLog2 (577-588), `X*(-1<<C) → (-X)*(1<<C)` Negator (293-303), `(zext X)*(-1<<C) → (zext (-X))<<C` (310-322), foldIDivShl all three variants (1206-1284), `1 << (cttz X)` (1377-1382).
- File: llvm/lib/Transforms/InstCombine/InstCombineShifts.cpp — focused on: `Shr (BinOp X Y), C` shifted-binop fold (724-792, 870-905), getShiftedValue Add poison-flag drop on lshift only (846-865), shl(shl(X,A),B) collapse + flag preservation (146-170), shl(shr X,C),C and (X>>C1)<<C ShrAmt<ShAmtC NUW-from-NSW special case (1199-1245), `(X+Y)/2 → X&Y` for 1-bit values (1422-1426), `(1 << ((BW-1)-X)) → SignMask >> X` (1371-1375), `X shift (Y|(BW-1)) → X shift (BW-1)` (552-553), `lshr (sext iM X to iN), N-M` and N-1 (1564-1580).
- File: llvm/lib/Transforms/InstCombine/InstCombineAndOrXor.cpp — focused on: foldLogOpOfMaskedICmps `setSameSign(false)` defensive drops on EQ pred (478-498), foldAndOrOfICmps `predicatesFoldable` mixed signed/unsigned path (3380-3401), foldXorOfICmps same (4725-4763), sinkNotIntoLogicalOp pre-guard against `Op0 = Not(Op1)` (4990-5021), foldComplexAndOrPatterns multi-clause (2025-2160).
- File: llvm/lib/Transforms/InstCombine/InstCombineSelect.cpp — focused on: foldSelectEqualityTest XeqY setSameSign(false) (1769-1794), canonicalizeSaturatedSubtract/Add predicate swap+arm swap (1038-1248), foldBitCeil getInversePredicate before arm swap (4053-4101), foldSelectToCmp pred flip and FlippedStrictness pair (4115-4250), canonical fcmp ugt→ole arm-swap (4516-4543), `(X >| 0) ? -X : X → fabs(X)` nnan/nsz gating (3300-3387), foldNegationOfSelected/SelectFromCmp pred swap.
- File: llvm/lib/Transforms/InstCombine/InstCombineVectorOps.cpp — focused on: collectSingleShuffleElements LHS/RHS same-type assertion (664-732), replaceExtractElements widening shuffle (737-810), evaluateInDifferentElementOrder including cast destination-type recompute (2009-2056), insertelement bitcast fold (1719-1776), foldShuffleWithBitcasts BegIdx alignment (3060-3128), insertelement-shuffle->shuffle mask construction.
- Patterns ruled out:
  - InstCombineAndOrXor `setSameSign(false)` after fold (478-498, 3420-3423, 1792): always semantically inert because each call site has ICMP_EQ/NE, where samesign doesn't change result (no consumer pattern matches `hasSameSign && ICMP_EQ` in upstream Analysis).
  - InstCombineShifts `(X|BW-1)` simplification (552-553): for any BW, `(X|BW-1) != BW-1` implies value ≥ BW (next "bit-superset" of BW-1 is ≥ BW even when BW is not a power of two); always UB→defined refinement or identity.
  - InstCombineShifts joint shl-shl collapse (146-170): NSW propagation requires both flags because outer-only NSW does not prevent the inner shift from poisoning, but combining can avoid the wrap that the inner shift would have exposed; verified by case analysis on i8 with X=2/A=4/B=1 and X=8/A=2/B=3.
  - InstCombineShifts canEvaluateShifted Add right-shift branch (695-718): only recurses when `WrapRequired` (NSW/NUW set on Add matching shift signedness), so each operand's shift-down preserves the wrap promise; getShiftedValue's no-flag-drop on Add for right-shifts is intentional and correct.
  - InstCombineMulDivRem `(X<<Y)/(X<<Z) → (1<<Y) >> Z` (1263-1281): NUW on the new `(1<<Y)` is sound because Y<BW is required for the original `X<<Y nuw` to not be UB; refinement-direction with X=0 (orig UB → new defined) is permitted.
  - InstCombineMulDivRem `(zext/sext X) * (-1<<C) → (zext (-X)) << C` (310-322): the `ShiftAmt >= BW-SrcWidth` gate ensures shifted-out bits don't matter, making zext/sext interchangeable; verified for sext path with X=INT_MIN of i4 → i8.
  - InstCombineMulDivRem `mul (shr exact X, N), (2^N+1) → add (X, shr exact X N)` (266-291): NSW on new add gated by NUW || LShr || ShiftC<BW-1; for AShr at BW-1 with positive X the add could overflow, hence the gate.
  - InstCombineAddSub `(X|C2)+C → X+(C2+C)` disjoint-or (917-925): NUW preserved unconditionally is sound because the new sum is bounded by the original (disjoint-or X|C2 ≥ X as unsigned, so original NUW implies X + truncated_constant fits as well).
  - InstCombineAddSub `(X<<S)+(Y<<S) → (X+Y)<<S` (1480-1507): triple-flag conjunction (outer + both inner shifts) is necessary; verified with i8 X=Y=0x10 S=4 where outer NSW alone admits non-NSW combined result.
  - InstCombineAddSub `add (sub X Y) -1 → add (not Y) X` (878-881): bit-identity `(X-Y)-1 = X + (-Y-1) = X + ~Y`, no overflow consequences.
  - InstCombineVectorOps collectSingleShuffleElements assertion `LHS->getType() == RHS->getType()` (666-667): guarantees `NumLHSElts == NumElts` so mask convention `Idx ∈ [NumLHSElts, 2*NumLHSElts) → RHS` matches shufflevector ABI.
  - InstCombineVectorOps insertelement bitcast fold (1763-1776): bitcasts must preserve scalar bitwidth, so VecSrc's element count equals IE's element count and IdxOp is reinterpretable across both vectors.
  - InstCombineSelect `(X ugt Y) ? X : Y → (X ole Y) ? Y : X` (4516-4543): table verified for NaN, X>Y, X<Y, X==Y cases.
  - InstCombineSelect foldSelectInstWithICmpConst `getSwappedPredicate(Pred)` at 2050 paired with `FlippedStrictness->second`: for ult+C0 the new `ugt(C0-1)` is exactly `!ult(C0)`; getSwappedPredicate paired with the flipped constant happens to equal getInversePredicate paired with the original constant (true for canonical predicate set used here).
- Potential bugs filed: none — none of the patterns I read have a confidently-incorrect rewrite in the bug categories listed.

## worker-53 2026-05-21
- File: llvm/lib/Transforms/Scalar/DeadStoreElimination.cpp:1-2799 — partial-overlap merge (2683-2708, helper 770-814), eliminateRedundantStoresViaDominatingConditions (2143-2250), storeIsNoop (2356-2428), eliminateDeadDefs (2591-2762), getDomMemoryDef (1627+), isRemovable (1452), isReadClobber (1579), isDSEBarrier (2080), tryToShorten/Begin/End (620-768), isShortenable* (191-220).
- File: llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp:1-2272 — processMemSetMemCpyDependence (1288-1384), processMemCpyMemCpyDependence (1102-1267), processMemCpy (1794-1918), processStore (745-825), processStoreOfLoad (631-743), tryMergingIntoMemset (352-501), performCallSlotOptzn (842-1098), processByValArgument/processImmutArgument (~2000-2200).
- File: llvm/lib/Transforms/Scalar/LoopIdiomRecognize.cpp:1-3589 — isLegalStore (459-571), processLoopMemCpy (816-876), processLoopMemSet (879-980), processLoopStridedStore (1052-1231), processLoopStoreOfLoopLoad (1236-1519), mayLoopAccessLocation (986-1050), MemmoveVerifier.
- Patterns ruled out:
  - DSE OW_Complete elimination path correctly excludes volatile/atomic DeadI via getDomMemoryDef→isRemovable (line 1733) and isRemovable(SI) → SI->isUnordered() (line 1457).
  - DSE storeIsNoop / eliminateRedundantStoresViaDominatingConditions both check SI->isUnordered() before eliminating; volatile/atomic stronger-than-monotonic are excluded.
  - DSE tryToShorten on atomic mem intrinsic gates NewSize % ElementSize (line 674-680); ToRemoveSize is implied a multiple of ElementSize.
  - MemCpyOpt processMemCpyMemCpyDependence checks MDep->isVolatile() (1106) and emits NewM with M->isVolatile() (already non-volatile per outer check at 1796).
  - MemCpyOpt processStore early-bails on `!SI->isSimple()` (746); processStoreOfLoad bails on `!LI->isSimple()` (634).
  - MemCpyOpt processLoopMemCpy bails on volatile (819) — non-element-atomic MemCpyInst doesn't carry atomicity.
  - LoopIdiom isLegalStore checks SI->isVolatile() (461), SI->isUnordered() (464), LI->isVolatile() (548), LI->isUnordered() (551); processLoopStoreOfLoopLoad asserts isUnordered (1238, 1246) and handles IsAtomic path (1425+) with atomic-memcpy intrinsic.
- Potential bugs filed:
  - candidates/w53-memcpyopt-memsetmemcpy-drops-volatile-memset.md — processMemSetMemCpyDependence (MemCpyOpt) never checks MemSet->isVolatile()/isAtomic(); confirmed via opt that a `volatile memset(32) + memcpy(16)` becomes `non-volatile memset(16)` (loses bytes 0..16 of volatile writes and drops volatile bit on tail); `volatile memset(N) + memcpy(N)` deletes the volatile memset outright.
  - candidates/w53-dse-partial-merge-drops-volatile-atomic.md — DSE partial-overlap store merging (OW_PartialEarlierWithFullLater branch at 2683) and tryToMergePartialOverlappingStores (770) never check KillingSI isSimple/isUnordered; confirmed via opt that `store i32 0; store volatile i16 -1` and `store i32 0; store atomic i16 -1 monotonic` are both merged into a single non-volatile/non-atomic `store i32 -65536`.

## worker-54 2026-05-21
- File: llvm/lib/Transforms/Scalar/Reassociate.cpp:1-2663 — full read; focused on
  OverflowTracking::mergeFlags/applyFlags (Utils/Local.cpp:4047-4074),
  RewriteExprTree flag-clear/apply loop (728-751), LinearizeExprTree mergeFlags
  (434), LowerNegateToMultiply (289-305), BreakUpSubtract (997-1017),
  ConvertShiftToMul (1021-1047), convertOrWithNoCommonBitsToAdd (950-964),
  OptimizeAdd factor extraction (1518), OptimizeMul/buildMinimalMultiplyDAG
  (1861-1889/1804-1858), OptimizeXor (1365-1482), canonicalizeOperands (236-248).
- File: llvm/lib/Transforms/Scalar/SCCP.cpp:1-140 — driver only; real logic in
  Utils/SCCPSolver.cpp (already audited by worker-33 for the freeze/poison pattern).
- File: llvm/lib/Transforms/Scalar/MergeICmps.cpp:1-925 — full read; focused on
  visitICmpLoadOperand isSimple()/dereferenceability gates (147,157),
  visitCmpBlock predicate selection, BCECmp swap-to-canonical-order (195),
  mergeComparisons clone-load preserving alignment/MMOs (683-689),
  canSinkBCECmpInst write-clobber detection (247-267), processPhi structural
  match (816-883).
- File: llvm/lib/Transforms/Scalar/AlignmentFromAssumptions.cpp:1-313 — full
  read; focused on getNewAlignmentDiff offset/AlignSCEV math (50-77),
  getNewAlignment AddRec start/inc-min path (108-156), extractAlignmentInfo
  bundle decoding (159-187), processAssumption GEP/PHI worklist recursion
  (215-278).
- Patterns ruled out:
  - Reassociate `OverflowTracking::applyFlags` (Utils/Local.cpp:4063) gates Add NSW
    on `(AllKnownNonNegative || HasNUW)` and Mul NUW/NSW on `AllKnownNonZero`;
    the gates are conservative-correct for tree reassociation (verified via
    case analysis of partial-sum overflow with negative leaves and Mul-with-zero
    edge cases). NUW preservation on Add inner-nodes is unconditional but sound
    because A+B+C fitting unsigned implies B+C fits.
  - Reassociate `convertOrWithNoCommonBitsToAdd` (950-964) unconditionally sets
    nsw+nuw on the new Add; correct because disjoint Or has X+Y == X|Y so no
    carry and no signed overflow regardless of MSB.
  - Reassociate `ConvertShiftToMul` (1040-1045) correctly preserves NUW
    unconditionally and NSW only when `(NUW || ShAmt.ult(BW-1))`.
  - Reassociate `RewriteExprTree` clears subclass data via
    `ExpressionChangedStart->clearSubclassOptionalData()` before reapplying
    OverflowTracking flags or root FMF; intermediate nodes that didn't change
    keep their original flags (the loop stops `ClearFlags=false` after
    `ExpressionChangedEnd`).
  - MergeICmps `visitICmpLoadOperand` correctly rejects volatile/atomic loads
    via `!LoadI->isSimple()` (line 147) AND requires unconditional
    `isDereferenceablePointer` (line 157) so reordering compare-blocks is sound.
    The "merges icmps differing in side effects" bug pattern is blocked.
  - MergeICmps `BCECmp` swap of (Lhs,Rhs) is sound because ICmpInst EQ/NE is
    symmetric.
  - AlignmentFromAssumptions `getNewAlignment` DiffSCEV math
    `(PtrSCEV - AASCEV) + OffSCEV` is correct given the align-bundle semantic
    `(AAPtr - OffSCEV) % Align == 0` (i.e. `AAPtr ≡ OffSCEV (mod Align)`);
    verified against the test/Transforms/AlignmentFromAssumptions/simple.ll
    `align(a, 32, 24)` / `align(a, 32, 28)` cases.
  - AlignmentFromAssumptions worklist walk through PHI/GEP users (267-277)
    relies on SCEV non-simplification for arbitrary PHIs (yields
    SCEVUnknown → Align(1)) so cross-path alignment leakage doesn't fire in
    practice; AddRec path correctly takes `min(NewAlignment, NewIncAlignment)`.
- Potential bugs filed:
  - candidates/w54-reassociate-fneg-to-fmul-snan.md — `LowerNegateToMultiply`
    (Reassociate.cpp:289-305) converts unary `llvm.fneg` to `fmul %x, -1.0`,
    quieting sNaN. Inline FIXME at line 292 documents this is unsafe. Reached
    from OptimizeInst FNeg arm (2226-2253) whenever the FNeg's operand is a
    reassociable FMul (carrying `reassoc nsz`); the FNeg's *own* FMF (no nnan
    required) is not gated. Direct miscompile candidate on x86 where FMUL
    lowers to mulss/mulsd (sNaN-quieting) while FNEG lowers to xorps
    (bit-preserving).

## worker-54 2026-05-21

Hunted for integer/bitwise/vector x86 miscompiles via InstCombine and codegen folds.
No confirmed miscompiles in ~10 min window. Patterns investigated:
- fshl/fshr edge cases (shift mod bw, i7 non-pow2, rotate idiom recognition)
- umul/smul.with.overflow with constants {-1, INT_MIN}
- uadd.sat / usub.sat / sadd.sat boundary saturation
- smin/smax chain invariants
- vpermilvar.ps selector masking (uses bits[1:0])
- vector.reduce.or with all-constant input
- sext/zext folds around lshr/ashr/icmp
- ctlz/cttz with is_zero_undef + select-on-zero
- vpternlog (skipped: intrinsic not folded, becomes extern call)
- smul.fix.sat lowering

All folds verified correct per LangRef. See w54-investigation-notes.md.
Notable: rotate idiom (shl|lshr with sub 32,n pattern) correctly recognized as
fshl despite original IR having UB at n=0 (refinement OK).

## worker-55 2026-05-21
Explored x86 vector/shuffle/mask miscompile patterns via end-to-end llc tests
(not source reading). Targets: shufflevector with undef/poison mask elements,
extractelement/insertelement chains across bitcast (i8<->v8i1, i64<->v64i1,
v4i32<->v8i16, i128<->v4f32), <n x i1> mask ops (and/or/xor/not, kshiftl/kshiftr
patterns), vselect with i1-vec masks, masked.load/store with all-true/all-false
masks (correctly folded to plain load/store/nop on both AVX2 and AVX-512),
masked.gather/scatter with constant masks, vp.add/mul/load/store with various
EVL (incl. EVL > vector length, EVL = 0 which makes result poison so any output
is allowed), vector.reduce.{add,mul,or,and,smax,umin,fadd,fmul} on NPOT vectors
(3, 5, 7 elements - correctly pad with identity element, e.g. all-ones for umin,
zero for add, etc.), AVX permutes with edge-case immediates (vpermilps with
indices >= lane size correctly masked to low bits, vpermpd zmm cross-lane,
pshufd $0xff, pblendw $170), x86 intrinsics (insertps with all-bits zmask
correctly produces zero, vcvtps2ph, vpalignr, sse2.pmovmskb), trunc+shuffle
across element widths (v4i32 -> v4i16 reverse via pshuflw/pshufhw chain
verified correct via element trace), FP arith with NaN constants, FP reduce
with start=-0.0 (correctly omits initial start-add: equivalent under IEEE-754
since -0.0+x=x for non-zero x and -0.0+(±0)=±0 in round-nearest), fadd of
all-zero vector preserved (not folded - correct: needs sign of zero), shl
of <n x i1> by all-ones folded to whatever-is-in-xmm0 (correct: shift-by-bitwidth
is poison per LangRef).

Negative findings (codegen behavior verified correct via element-level trace
of asm): red_mul_i8 zext-promote-to-i16-then-mul + low-byte-extract preserves
i8 wrap semantics (a*b mod 256) * (c*d mod 256) mod 256 = a*b*c*d mod 256.
red_fadd_3 v3f32 ordered reduce: scalar addss sequence correctly accumulates
0+v[0]+v[1]+v[2] using movhlps trick for v[2]. v3i32 reduce_add/mul/smax/umin
correctly initializes padding lane with identity. ucmp on <8 x i1> correctly
extracts even bytes (i1 stored as i16 in xmm) and ANDs with 1 before scalar
compare.

No confirmed reproducible miscompiles filed. All AVX-512 mask ops, masked
memory ops, VP intrinsics, x86 shuffle intrinsics, and FP-reduce variants
behaved correctly under llc -O2. Limited time spent reading source files
- focus was end-to-end llc behavior with ~50 small IR snippets across
mcpu=skylake and mcpu=skylake-avx512.

## worker-53 2026-05-21 (FP-related second pass)

Scope: FP folds in DAGCombiner.cpp visitFADD/visitFMUL/visitFDIV/visitFSUB/visitFMA, simplifyFPBinop, IR-level simplifyFMul/simplifyFDiv, FCOPYSIGN/FCMP folds; X86InstCombineIntrinsic.cpp scalar/vector intrinsic combines.

Files actively read:
- llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:18800-20554 (re-read FP combine block; lines 19040-19110 FSUB/FMUL identity folds, 19203-19500 visitFMUL/visitFMA, 19590-19733 visitFDIV/visitFREM, 19827-19866 visitFCOPYSIGN, 20225-20475 visitFP_ROUND/visitFP_EXTEND/eliminateFPCastPair/visitFFLOOR/visitFTRUNC/visitFNEG/visitFMinMax)
- llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp:11584-11627 (simplifyFPBinop)
- llvm/lib/Analysis/InstructionSimplify.cpp:6054-6200 (simplifyFMAFMul/simplifyFDivInst/simplifyFAdd/simplifyFSub)
- llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp:1737-1783 (simplifyX86FPMaxMin), 2351-2590 (cvt intrinsics, comieq/ucomieq, mask_add_ss_round/etc., max_ss/min_ss/min_pd), 2955-3170 (blendv, vpermilvar, maskload/maskstore), 3175-3210
- llvm/lib/Transforms/InstCombine/InstCombineAddSub.cpp:3049-3234 (foldFNegIntoConstant, visitFNeg, visitFSub) and InstCombineCalls.cpp:3056-3196 (copysign), 8910-8990 (foldFCmpWithFloorAndCeil)
- llvm/lib/Target/X86/X86ISelLowering.cpp:23370-23429 (LowerFABSorFNEG), 273-340 (PR44019 FIXMEs, strict_fp_to_si8 promote)

Patterns ruled out / verified safe:
- visitFCOPYSIGN `copysign(x, fp_extend(y))` -> `copysign(x, y)`: fp_extend preserves sign bit; safe.
- visitFCOPYSIGN sign-mask -> disjoint-or (19848-19863): only fires when N0 sign-bit known zero.
- visitFTRUNC `trunc(trunc x)` / `trunc(floor x)` / `trunc(ceil x)` -> single rounding (20410-20420): idempotent; sNaN quieting preserved.
- visitFP_ROUND/visitFP_EXTEND eliminateFPCastPair (20293-20326): correctly requires both nnan+ninf and contract on both casts before eliminating the pair.
- foldFCmpWithFloorAndCeil (8910-8985): `fcmp ogt floor(x), x => false` etc. verified for NaN/Inf cases.
- visitFMA `fma(1.0, X, Y) -> fadd X, Y` (19395-19398): X*1.0 is exact, sNaN quieted by fadd; semantically safe.
- visitFMA `fma(X, -1.0, Y) -> fadd Y, fneg(X)` (19432-19437): X * -1.0 is exact, sNaN quieted by fadd; safe.
- visitFREM pow2 expansion (19752-19772): `x - trunc(x/c)*c` with `copysign(result, x)` correctly quiets sNaN through the implicit FDIV/FMUL/FSUB chain.
- InstCombine fneg(copysign(x,y)) -> copysign(x, fneg(y)) (3211-3220): correct.
- InstCombine foldFNegIntoConstant `-(X / C) -> X / -C` (3072-3084): correct for NaN/Inf C; intersects nsz/ninf properly.
- X86InstCombineIntrinsic simplifyX86FPMaxMin (1737-1783): correctly requires both operands known-never NaN+Inf+Subnormal before lowering SSE max_ps/max_pd to llvm.maxnum (forbidden NegZero asymmetry per min/max is intentional).
- X86InstCombineIntrinsic mask_add_ss_round et al. (2487-2549): preserves Arg0's upper 3 vector lanes via InsertElement; masking via select correctly preserved.
- LowerFABSorFNEG (23370-23429): the xorps sign-bit lowering is correct for FNEG in isolation; FNEG IR semantic is exactly a sign-bit flip per LangRef.
- PR44019 (strict_fp_to_si8 promote): the missing-INVALID-exception case requires fenv observability, hard to demo as a value miscompile.

Potential bugs filed:
- (removed) two sNaN-quieting candidates: `fdiv X, -1.0` xorps chain and `simplifyFPBinop` `X*1.0` / `X/1.0` identity folds. Dismissed as pattern: LangRef does not require quieting on these unguarded paths, and the broader "LLVM doesn't quiet sNaN" issue is known and not worth filing in isolation.

Already covered by earlier workers (not re-filed):
- visitFSUB `(fsub -0.0, X) -> fneg X` FIXME at line 19057 — w12-fsub-negzero-fneg-snan.md
- visitFMUL `(fmul X, -1.0) -> (fsub -0.0, X)` — w12-fmul-neg1-fsub-snan.md
- visitFMinMax MINIMUMNUM(X, NaN_const) sNaN passthrough — w12-fminimumnum-snan-not-quieted.md
- Reassociate LowerNegateToMultiply fneg->fmul sNaN — w54-reassociate-fneg-to-fmul-snan.md
- AVX512 round-mode 4 (CUR_DIRECTION) reduce to fadd/etc dropping MXCSR — w04-avx512-add-ps-512-cur-direction-MXCSR.md

## worker-56 2026-05-21
- Approach: differential testing via `clang -O0` vs `clang -O3`, hand-written C and LLVM IR. No new candidates filed.
- Patterns tested (no NEW divergence beyond what is spec-permitted or already filed):
  - Funnel-shift `llvm.fshl.i{8,16,32,64}` at amounts 0, 1, BW-1, BW, BW+1, BW*2, modulo behavior — clean.
  - `__builtin_clz/ctz(0)` (always wrapped in safe `x?clz:32` form) — clean.
  - `fmin/fmax` (scalar f32/f64) with QNaN, SNaN, +/-0 inputs — clean (NaN bit/sign nondeterminism per LangRef).
  - `__builtin_fma(0, inf, -inf)` and other `0*inf + finite` cases — clean.
  - Vector reductions `__builtin_reduce_{max,min,add}` over i8/i32 with negatives — clean.
  - i128 arithmetic incl. `*`, `/`, `%` with extreme values, prime-ish divisors — clean.
  - 64-bit pointer arithmetic with `int64_t` offsets — clean.
  - TLS read/write with `local-exec/initial-exec/local-dynamic` models — clean.
  - Atomic RMW orderings (`seq_cst/release/acquire/acq_rel/relaxed`) — clean.
  - x86 BMI: `pext/pdep/lzcnt/tzcnt/blsi/blsmsk/blsr/bzhi(idx>=64)/mulx(-1,-1)` — clean.
  - x86 PSHUFB ctrl high-bit zeroing, PSLLW/PSRAW count>=BW saturation, PMOVMSKB — clean.
  - Saturating PADDS/PSUBS/PADDUS/PSUBUS at INT*_MIN/MAX boundaries (incl. PMADDWD `(-32768)*(-32768)` overflow) — clean.
  - PMULDQ/PMULUDQ/PMULLD with INT_MIN inputs — clean.
  - `__builtin_{add,sub,mul}_overflow` at INT_MIN*-1, INT_MAX*2, UINT_MAX+1 — clean.
  - Bitfield struct stores/loads (5/7/11/9 split across 32-bit, 33/30 across 64-bit) — clean.
  - bswap / DAG-detected BE-load combine (16/32/64) at aligned/unaligned offsets — clean.
  - Mis-aligned narrow loads of wider stored values (u32@1,4,7; u16@0,7) — clean.
  - memmove with overlap (forward by 1, backward by 1 over 64B) — clean.
  - Switch lowering across small (uint8 ranges) and sparse (INT_MIN, INT_MAX, 100000) — clean.
  - Vector i32x4 shifts at amounts 0..1000 — clean (modulo poison amounts).
  - x86 AVX-512 masked load/store / maskz_loadu_epi32 with passthru — clean.
  - cmpxchg @ 16/32/64 bit success+fail paths — clean.
  - setjmp/longjmp interaction with volatile locals — clean.
  - Long-double / x87 80-bit catastrophic-cancellation patterns — clean.
  - Bitfield/alignment math `(p + a - 1) & ~(a-1)` — clean (ASLR noise initially confused).
  - Comparison chains `(x>=0 && x<100)` vs `((unsigned)x<100u)` — clean.
  - volatile vs relaxed-atomic loop reads — clean.
- Spec-permitted divergences observed (NOT miscompiles):
  - `fmul X, -1.0` / `fsub -0.0, X` / `X*1.0` / `X-0.0` / `X/1.0` / `(float)(double)snan` drop SNaN-quieting at -O3.
    LangRef line 4119-4120 explicitly permits `fmul SNaN, 1.0 -> SNaN`. Already filed:
    `w12-fsub-negzero-fneg-snan.md`, `w12-fmul-neg1-fsub-snan.md`, `w54-reassociate-fneg-to-fmul-snan.md`.
  - `inf - inf` returns differently-signed NaN at O0 vs O3 (`ffc00000` vs `7fc00000`). LangRef nondeterministic NaN sign.
  - `(unsigned)(-1.5)` differs between O0 (truncated bits) and O3 (folded to 0 via `foldFPtoI`). LangRef poison for out-of-range fptoui. Already analyzed by w29.
- Bugs filed: none new (all observed divergences are already-filed or LangRef-permitted).

## worker-60  2026-05-21
- File: llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp — FIXME mining (60 hits scanned, ~head 50);
  focused reads:
  - visitFMinMax (20477-20554): re-verified per LangRef that minimumnum(X, sNaN) -> X is spec-conformant
    (LangRef: "If one operand is NaN (including sNaN) and another operand is a number, return the number").
  - visitFP_ROUND (20225-20290): confirmed bug-112 root (line 20236, `fp_round(fp_extend x) -> x`).
  - visitFMUL (19203-19272): line 19266 already filed as bug-035.
  - visitFMA (19350-19460): `fma 1, X, Y -> fadd X, Y` — same NaN/Inf behavior as fma; not a bug.
  - visitFNEG (20445-20475): line 20462 `nsz` guard correct; FIXME just documents already-correct check.
  - visitFCOPYSIGN (19827-19866): all paths sign-bit-only; safe for sNaN.
  - foldSignChangeInBitcast (30376-30413): sign-bit XOR/AND on integer payload; doesn't quiet but
    fneg/fabs are not required to quiet either.
- File: llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp:11584-11627 (simplifyFPBinop):
  found additional unguarded identity arms (FADD `X + -0.0 -> X`, FSUB `X - +0.0 -> X`) that
  bypass sNaN quieting. The FMUL/FDIV arms were already covered by w53.
- Patterns ruled out (Strategy 3, integer boundary):
  - sdiv/srem at INT_MIN, -1, 0 boundaries — clean (UB-based folds well-formed).
  - shl/lshr/ashr at overshift (32 on i32) and constant amounts — clean.
  - fshl/fshr with constant amounts (including 0 and >width) — clean.
  - or disjoint chains, xor cancellation chains, mul x,0/1/-1, sub x,x, add nuw x,-1 — clean.
  - setcc add/mul/shl C2 canonicalization (add 5,==10 -> x==5 etc.) — clean.
  - vector ext_concat / double_insert / fshl with mixed shift amounts — clean.
  - sext(icmp slt x, 0) and zext branchless masking — clean.
- Patterns ruled out (Strategy 2, FP at NaN/0/Inf):
  - fp_extend / fp_round / fp_round-of-fp_round with sNaN — only `fp_round(fp_extend x)` is broken
    (already bug-112).
  - fceil/ffloor/sqrt/fcanonicalize with constant or sNaN — clean (no constant fold without flags).
  - fcmp ord/uno/oeq/one/ueq with constant sNaN or self — clean (LangRef permits any qNaN-equivalent result).
  - strict_fmul X, 1.0 and strict_fadd X, -0.0 — correctly NOT folded (strict path).
  - constant-fold of fadd/fmul/fsub/fdiv with sNaN constants — correctly quiets to qNaN.
  - fp_to_si/fp_to_ui of sNaN — defined as poison per LangRef.
  - half<->float<->double round-trips (with f16c) — clean.
- Potential bugs filed:
  - (removed) sNaN-quieting bypass candidate for simplifyFPBinop FADD/FSUB arms. Same dismissal as the w53 FMUL/FDIV siblings — "LLVM doesn't quiet sNaN" is a known limitation, not worth filing.
- Note: tried to file `fp_round(fp_extend)` candidate but found bug-112 already exists; removed. (bug-112 has since also been removed for the same reason.)

## worker-58 2026-05-21
- File: llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp:1-3540 — full read; focused on simplifyX86immShift / simplifyX86varShift (out-of-range and UNDEF handling), simplifyX86pack (PACKSS/PACKUS clamp + shuffle), simplifyX86pmulh (multiply-by-one / by-undef), simplifyX86pmadd (PMADDWD / PMADDUBSW with sadd_sat), simplifyX86FPMaxMin (KnownFPClass forbidden classes), simplifyX86insertps, simplifyX86vpermilvar (PD lshr-1 bit-1 extraction), simplifyX86vpermv / vpermv3 (Size-1 / 2*Size-1 masking), simplifyX86VPERMMask (log2 demanded bits), simplifyX86movmsk, simplifyTernarylogic (full truth table), x86_avx512_mask_{add,sub,mul,div}_{ss,sd}_round CUR_DIRECTION path, x86_avx512_{add,sub,mul,div}_{ps,pd}_512 CUR_DIRECTION path, pclmulqdq demanded-elt path, BMI bextr/bzhi/pext/pdep folds, addcarry m_ZeroInt() carry-in.
- Verified via opt -passes=instcombine -S with x86_64-unknown-unknown triple:
  - pmulh.w / pmulhu.w / pmul.hr.sw constant folding for extreme inputs (-32768, 32767) — all correct, including pmulhrsw rounding overflow at -32768 * -32768.
  - pmaddubsw saturation at 255 * -128 + 255 * -128 = -32768 (saturated) — correct.
  - pternlogd 0x96 (A^B^C), 0xff (all-ones), 0xa5 (Xnor(A,C)) — all correct.
  - vpermilvar.pd.512 bit-1 extraction with constants 1/3/5/7 (bit 0 ignored) — correct.
  - vpermb / vpermd / vpermilvar.ps.512 — all correct lane-aware shuffles.
  - addsub.ps demanded-elt fold to fadd/fsub — correct (sub on even lanes, add on odd lanes).
  - bextr/bzhi length=0 / shift>=BitWidth — correct (returns 0 / returns input).
  - psllv/psrlv/psrav out-of-range single-element bailout (AnyOutOfRange) — safe.
  - pslli.w shift=16, pslli.q shift=64, pslli.w shift=-1 (= 0xFFFFFFFF) — all correctly return zero.
  - psrav.w arithmetic with shift>=16 splat sign — correct.
  - pternlog with imm>=256 — bailout at line 673 preserves semantics; LLC encodes correctly as low-8-bit imm.
- Patterns ruled out:
  - VPERMILVAR.PD bit-1-only demand at lines 3110-3112 (APInt(64, 0b00010)) and simplifyX86vpermilvar lshrInPlace(1) are consistent — no bit-0 leak.
  - simplifyX86pmulh "multiply by one" m_One() correctness for both signed (AShr 15) and unsigned (zero) — match Intel spec.
  - simplifyX86vpermv3 index masking (2*Size-1) handles all byte/word/dword/qword vperm2 widths correctly.
  - simplifyTernarylogic case 0x96 (A^B^C), 0x69 (Xnor(A,B)^C), 0xa5 (Xnor(A,C)), 0x55 (Not(C)) — truth tables verified.
  - simplifyX86FPMaxMin Forbidden classes (NaN/Inf/Subnormal + NegZero per side) correctly avoid SSE-vs-minnum disagreement when fold fires.
- Potential bugs filed:
  - candidates/w58-avx512-mask-arith-ss-sd-round-CUR_DIRECTION-MXCSR.md — x86_avx512_mask_{add,sub,mul,div}_{ss,sd}_round at lines 2487-2549 fold to plain extract/fadd/insert when rounding-mode imm == 4 (CUR_DIRECTION). Sibling of w04 but distinct switch arm targeting the *masked scalar* round variants. Demonstrated reproducer that drops MXCSR rounding when ldmxcsr is in scope.

## worker-59 2026-05-21
- Hunt: x86 `-O0 -global-isel` runtime miscompiles (target patterns GISel handles itself).
- Approach: write small IR per hot target, compile both `-O0` vs `-O0 -global-isel`,
  link with a C driver, diff runtime output.
- Patterns exercised (all matched SDAG runtime, ruled out as miscompile sources):
  - i64 mul/udiv/sdiv/urem, i64 bswap.
  - icmp ule/sle/ult/ugt/eq/ne with various widths; sub-then-eq0; trunc-to-i1 branch;
    select with two loads; shift with i8/i64 amount; ashr i64; manual rotl pattern.
  - sub i128, sub i256, sub i128 const subtrahend — REPRODUCES bug-110
    (`selectUAddSub` borrow inversion via `setb; cmpb $1`); duplicate, candidate removed.
  - fptosi/fptoui, sitofp/uitofp (i32, i64), fcmp ueq/oeq/uno/ord/uge,
    sqrt/fabs, fneg, fsub -0.0,x, fadd/fdiv f32 + f64.
  - sext/zext i8->i64, sext_i1, zext_i1, manual extend loads, trunc-store-i8,
    trunc-sext round-trip.
  - signed/unsigned div+rem, sdiv 8-bit, sdiv by pow2, sdiv by neg const.
  - atomic load/store seq_cst i32 (atomicrmw, cmpxchg fall back).
  - volatile load/store; load-modify-store; multi-block PHI chain; switch with
    case 1/2/100/default; conditional branches; and/or used as branch condition.
  - uadd.with.overflow.i64 and i32 carry-flag used as zext-i64 add — clean.
  - usub.with.overflow.i64 carry-flag used as zext-i64 add — clean.
  - usub.with.overflow.i64 flag used as br i1 — clean.
  - many_args (8 i32, stack), mix_args (mixed i8/i64), struct sret 24-byte.
  - shl/lshr/ashr by i64 constant (33), bswap16/32, manual rotl.
  - large i64 constants (2^32, ~-1), or/xor with big const, sub from constant.
  - global var load/store, const array index, FP load/store, bitcast f32<->i32,
    ptrtoint/inttoptr.
  - tail/musttail call, memcpy.p0.p0.i64, memset.p0.i64.
  - gep i8 + offset, gep with sext i32 idx; alloca; struct field GEP w/ array.
- GISel legalization holes (not bugs, but skipped — fall back to fatal error at -O0):
  - G_CTLZ/G_CTTZ/G_CTPOP i32, G_FSHL/G_FSHR (rotates), G_SADDSAT/G_USUBSAT,
    G_ABS, G_SMIN/G_SMAX/G_UMAX (work at -O0 actually — re-checked, fine),
    G_ATOMICRMW_ADD i32, G_ATOMIC_CMPXCHG_WITH_SUCCESS i32, G_SSUBO i64,
    G_UMULO i32, inline asm in IRTranslator (fatal report_fatal_error).
- New bugs filed: 0 (one candidate written for the i128 sub borrow inversion but
  removed after finding it is bug-110).

## worker-62 2026-05-21

Scope: x86_64 differential testing via `llc -O0` / `llc -O2` / `llc -global-isel`
on small IR snippets across multi-word integer arithmetic, saturating arithmetic,
funnel shifts at OOB amounts, ctlz/cttz at zero (i32/i64/i128), FP NaN through
fcmp/fmin/fmax/fminimum/fmaximum/fminimumnum/fmaximumnum, vector reduce
(int/FP/NaN), TLS access, atomic loads/stores/cmpxchg/atomicrmw, variadic
printf with __int128/long double/i32, ptrtoint+inttoptr round-trip, bitcast
i64 <-> double sNaN, abs i32/i64/i128, smul/umul with overflow, sshl_sat/ushl_sat,
sel-abs / cond-neg / signmask / copysign, FP fast-math reassoc/X-X/X+(-X), and
GISel-specific i128 / i129 / i65 / phi-i128 patterns.

Confirmed runtime miscompile filed:
- `candidates/w62-gisel-add-i128-inverted-carry.md` — x86_64 GISel runtime
  witness for the bug already documented as w14 (i386) and bug 110 (sub128
  borrow). `llc -O0 -global-isel` lowers `add i128 %a, %b` to
  `addq lo; setb %sil; cmpb $1,%sil; adcq hi` — the `cmpb $1,%sil` inverts
  CF (1-1=0 vs 0-1=1), so the high half is always off by +/-1 in the wrong
  direction. Witnessed at `add128(1<<64, 1)`: SDAG -O0/-O2 return hi=1 lo=1;
  GISel returns hi=2 lo=1. Same pattern triggers on `add i128 %a, 0`,
  `sub i128 %a, 0`, `sub i128 %a, 1`, and the trunc-of-`lshr 64` extract of
  the high half. Symmetric counterpart to bug 110 sub128 already on disk.

Patterns ruled out / verified clean across O0/O2/GI (x86_64):
- fshl/fshr i32/i64 at shift amounts {0, BW-1, BW, BW+1} — correctly
  modulo-BW reduced.
- ctlz/cttz i32/i64/i128 with is_zero_undef=false: correct BW value at 0,
  correct otherwise.
- llvm.sadd.sat/ssub.sat/uadd.sat i32/i64/i128 at INT_MIN/INT_MAX/-1/UMAX
  boundaries: all three configurations identical and correct (the inverted-
  carry bug does *not* fire through the sat lowerings — uses different path).
- mul i128 by 2 across the 64-bit boundary, shl/lshr/ashr i128 at 64/127,
  sext i64->i128, and(i128), bitcast i64<->double sNaN round-trip:
  bit-exact across all configs.
- llvm.sadd/ssub/uadd.with.overflow i32, explicit uaddo/usubo i64 chains
  (manual 128-bit add via i64+i1): all identical and correct — only the
  IR-level `add/sub i128` direct form triggers the GISel CF inversion.
- TLS thread_local global load/store, atomic load/store/cmpxchg/atomicrmw
  add+0/seq_cst, volatile load, mixed atomic+plain stores to same address.
- ptrtoint/inttoptr round-trip on global, load/store via reconstructed pointer.
- abs i32/i64/i128 at INT_MIN; smax/smin/umax/umin; llvm.ucmp/scmp.
- llvm.bitreverse.i32, bswap.i64, ctpop i32/i128.
- variadic printf with __int128, long double (x86_fp80), and 6+i32 args:
  ABI-correct across all three.
- sel-abs / cond-neg / signmask / copysign for normal+NaN.
- FP min/max with +/-0 sign — minnum/maxnum result is operand-order-dependent
  (known IEEE-754 spec slack), not a miscompile; minimum/maximum follow 2019
  rule (-0 < +0) consistently.
- fptosi.sat/fptoui.sat with NaN/Inf/out-of-range inputs.
- v4i32/v16i8 vector shuffle reverse, reduce.add: correct.
- vector fcmp + reduce.or/and (ABI-bound observation only; values agree
  across all three configs).

FP sNaN passthrough observations (pre-existing, not re-filed):
- `fsub x, 0.0` is NOT folded at -O0 (real subq quiets sNaN) but IS folded
  at -O2 (sNaN passes through unchanged) — observable strict-vs-relaxed cliff
  that matches the documented w12/w53 family. Same pattern for
  `fmul x, 1.0` and `fadd x, -0.0`. With `nsz nnan`, `sub_self → 0` fold is
  taken at -O2 (correct per LangRef since `nnan` permits the optimizer to
  assume non-NaN input).

GISel selector aborts encountered (assertion failures, not miscompiles):
- `add i65 %a, %b`, `add i129 %a, %b`: `LLVM ERROR: unable to legalize
  instruction` — GISel falls back; test framework auto-falls-back to SDAG.
- `phi i128`: `cannot select G_UNMERGE_VALUES %vreg(s128)` — same fallback.
- Pass-by-`byval i128` / `sret i128`: same.

Files actually touched:
- New candidate: `/home/orenamd@semianalysis.com/FuzzX/x86/candidates/w62-gisel-add-i128-inverted-carry.md`
  (x86_64 runtime witness; cross-references w14 and bug 110)
- Read: `/home/orenamd@semianalysis.com/FuzzX/x86/candidates/w14-uadde-cmp-inverted-carry.md`
- Read: `/home/orenamd@semianalysis.com/FuzzX/x86/bugs/110-gisel-usube-inverted-borrow-sub128/{repro.ll,runner.c,cmd.sh}`

## w57: more "drops volatile / atomic / ordering" hunts

Confirmed-reproducible candidates filed:
- `w57-loweratomic-drops-volatile-on-rmw.md` — `lower-atomic` lowers
  `atomicrmw` and `cmpxchg` into raw load+store, silently dropping
  `volatile`, ordering, and syncscope. (`LowerAtomic.cpp::buildCmpXchgValue`
  and `lowerAtomicRMWInst`)
- `w57-gvnsink-merges-two-volatile-stores.md` — `gvn-sink` value-numbers
  volatile stores; two identical volatile stores in sibling blocks become
  one. (`GVNSink.cpp::ValueTable::createMemoryExpr`)
- `w57-simplifycfg-hoist-merges-two-volatile-loads.md` — `simplifycfg<hoist-common-insts>`
  hoists two identical volatile loads. (`SimplifyCFG.cpp::shouldHoistCommonInstructions`)
- `w57-simplifycfg-sink-merges-two-volatile-stores.md` — `simplifycfg<sink-common-insts>`
  sinks two identical volatile stores.
- `w57-simplifycfg-hoist-merges-two-seqcst-atomic-loads.md` — same hoist path,
  but for `seq_cst` atomic loads, which changes the global seq_cst total order.

Cleared (verified to be properly guarded — do not re-check):
- `SROA.cpp` — all rewrite paths guarded by `isSimple()` or have
  `assert(!isVolatile())`; presplit also guards.
- `InstCombineLoadStoreAlloca.cpp::unpackLoadToAggregate` (guarded by
  `!LI.isSimple()`). `unpackStoreToAggregate` *looks* unguarded but the
  end-to-end test of `store volatile {i32,i32} %v, ptr %p` shows the
  volatile store survives — some other condition I didn't find bails for
  volatile aggregate stores; not a confirmed miscompile.
- `LoadStoreVectorizer.cpp` — `!isSimple()` guard at line 1744.
- `ArgumentPromotion.cpp::HandleEndUser` — `!isSimple()` guard.
- `MergedLoadStoreMotion.cpp` — `!S0->isSimple()` guard in `mergeStores`.
- `GVNHoist.cpp::LoadInfo/StoreInfo::insert` — `isSimple()` guards.
- `JumpThreading.cpp::simplifyPartiallyRedundantLoad` — `isUnordered()` guard.
- `GVN.cpp::eliminatePartiallyRedundantLoad` — propagates volatile/ordering
  correctly into the new `LoadInst` ctor.
- `LICM.cpp` promotion — tracks `SawUnorderedAtomic` and excludes volatile.
- `Scalarizer.cpp::visitLoadInst/visitStoreInst` — `isSimple()` guard.
- `LoopLoadElimination.cpp` — LAA already filters non-simple ops before
  candidates are formed.
- `InterleavedLoadCombinePass.cpp` — `isVolatile()/isAtomic()` guards line
  873-876.
- `InterleavedAccessPass.cpp` — `isSimple()` guards everywhere.
- `Sink.cpp` — does NOT have a volatile-load guard, but in practice the
  CFG conditions for sinking a load (single-pred successors etc.) prevent
  the obvious volatile-load-loss repro I tried; couldn't reproduce.
- `VectorCombine.cpp::foldSingleElementStore` — `isSimple()` guard.
- `AggressiveInstCombine` — both load merge and store merge paths gated by
  `isSimple()`.
- `PromoteMemoryToRegister.cpp` — `isVolatile()` rejection.
- `GlobalStatus.cpp` + `GlobalOpt.cpp::TryToShrinkGlobalToBoolean` —
  `GlobalStatus::analyzeGlobal` bails on any volatile use, so subsequent
  optimization sees only non-volatile globals.
- `LowerMemIntrinsics.cpp` — propagates `SrcIsVolatile` / `DstIsVolatile`
  correctly into the loop and residual paths.
- `LowerMatrixIntrinsics.cpp` — propagates `isVolatile()`.


## worker-63 2026-05-21
- Goal: REPRODUCIBLE x86 lowering miscompiles in X86ISelLowering.cpp / X86InstCombineIntrinsic.cpp.
- Approach: read the intrinsic combiner tables (vpermilvar, vpermv, vpermv3, vpernlog/ternlogd, pmadd, pmulh, pack, addcarry, ROUND_SS/SD, MASKED_LOAD/STORE), spot-check x86 lowering paths (LowerMLOAD/MSTORE/MGATHER/MSCATTER, LowerFLDEXP, LowerINSERT_VECTOR_ELT, LowerSCALAR_TO_VECTOR, LowerEXTRACT_VECTOR_ELT_SSE4, LowerCMP_SWAP, LowerADDSUBO_CARRY, getPMOVMSKB, insert1BitVector, LowerBUILD_VECTORvXi1, combineX86CloadCstore, combineSetCCAtomicArith, combineSubABS / combineSub, combineCarryThroughADD, combineAddOrSubToADCOrSBB, combineCMov, combineVPDPBUSDPattern, combineLogicBlendIntoConditionalNegate, combineX86ShufflesConstants, simplifyTernarylogic table for all 256 imms).
- Patterns ruled out (with concrete tests):
  - simplifyTernarylogic (X86InstCombineIntrinsic.cpp:669-1734): generated all 256 imms with A=0xF0/B=0xCC/C=0xAA splats (truth-table convention where A is high bit of index), ran instcombine, compared each folded result byte against `imm`. **All 256 match.**
  - simplifyX86vpermilvar pd 256 with mixed positive + negative i64 mask (-1, 1, 2, 3): IC fold matches O0 lowering (shuffle to [1,0,3,3]).
  - simplifyX86pshufb with byte -1 (MSB set) in mask: IC correctly inserts 0 at that lane; matches O0 lowering.
  - simplifyX86pmulh.w (signed PMULH) with splat-1 const: fold to `ashr a, 15` is correct (PMULH of a*1 = sign byte).
  - simplifyX86pmulhu.w (unsigned PMULH) with splat-1 const: fold to zeros is correct.
  - LowerEXTRACT_VECTOR_ELT_SSE4 i8/f32/i32/i64 paths reviewed; PEXTRB with TRUNCATE produces correct GR8 sub-reg.
  - combinevXi1ConstantToInteger treating undef-as-zero (line 46527) is conservative but not unsound (undef may be any value, including 0).
  - LowerCMP_SWAP register copy + glue + LCMPXCHG_DAG chain wiring correct.
  - getPMOVMSKB v64i8 splits: `Lo = ZEXT`, `Hi = ANY_EXT << 32` then OR. ANYEXT upper bits become bits 64..95 which are gone in i64, so no garbage in result; correct.
  - simplifyX86pack signed/unsigned clamp via CreateICmpSLT + CreateSelect: signed PACK clamps [INT16_MIN, INT16_MAX] correctly; unsigned PACK clamps [0, 0xFFFF] correctly (SLT 0 catches negatives, SGT 0xFFFF catches >max).
  - simplifyX86pmadd (PMADDWD and PMADDUBSW): the signed/unsigned extend pattern and sadd_sat for PMADDUBSW match hardware spec; PMADDWD non-saturating add matches.
- Potential bugs filed: 0 new. The fldexp v8f16/v16f16 no-FP16 path silently feeds integer Exp to SCALEF (same bug pattern as w02-fldexp), but is already covered by w02-fldexp-missing-sint-to-fp-widened.md which explicitly lists that fall-through path.


## worker-61 2026-05-21
- Goal: find more passes that drop volatile/atomic on load/store, with `opt`-diff evidence.
- Approach: enumerate all `CreateAlignedLoad/Store`, `CreateLoad/Store`, `new LoadInst/StoreInst` in `llvm/lib/Transforms` + `llvm/lib/CodeGen`; check each call's upstream gate (isVolatile/isAtomic/isSimple/isUnordered) and downstream propagation; build IR repros and run `opt -passes=<pass> -S`.
- Files examined:
  - `llvm/lib/Transforms/Scalar/SROA.cpp` 2016-2080 (isVectorPromotionViableForSlice), 2320-2410 (isIntegerWideningViableForSlice), 2880-2960 (tree-structured merge guard), 3025-3055 (tree-merge rewrite), 3100-3215 (rewriteIntegerLoad/rewriteVectorizedLoadInst/visitLoadInst paths), 3250-3390 (rewriteVectorizedStoreInst/rewriteIntegerStore/visitStoreInst), 1555-1640 (PHI speculation), 1720-1810 (select speculation). The viability checks at 2054/2067/2349/2375 guard only on `isVolatile()` and never on `isAtomic()`; the load/store rewrite synthesizers at 3166/3205/3371 then conditionally propagate atomic ordering **only** if `LI.isVolatile()` / `SI.isVolatile()` is true. Result: an atomic-but-not-volatile (e.g. `unordered` / `monotonic`) load or store has its atomic ordering silently discarded when SROA goes through vector- or integer-widening slice rewriter.
  - `llvm/lib/Transforms/Utils/SimplifyCFG.cpp` 4269-4429 (mergeConditionalStoresEdge/mergeConditionalStores) and 6985-6995 (switch-to-lookup-table load). The filter at 4275 uses `isUnordered()` which excludes volatile but accepts Unordered atomic; the merged store at 4408 is created via plain `QB.CreateStore` with no `setAtomic` follow-up.
  - `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp` 590-650 (combineLoadToNewType / combineStoreToNewValue both propagate volatile + atomic), 730-840 (unpackLoadToAggregate gated by `LI.isSimple()`), 1080-1180 (visitLoadInst -> load(select), load(GEP) gated by `isUnordered()`), 1320-1430 (unpackStoreToAggregate gated by `SI.isSimple()`), 1700-1740 (visitStoreInst MergedVal new StoreInst propagates volatile+atomic).
  - `llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp` 170-285 (memcpy/memset expansion both propagate volatile + atomic), 290-380 (masked_load / masked_store / masked_gather: intrinsics carry no volatile/atomic, so n/a).
  - `llvm/lib/Transforms/InstCombine/InstructionCombining.cpp` 4810-4845 (extract-of-load gated by `L->isSimple()`).
  - `llvm/lib/Transforms/AggressiveInstCombine/AggressiveInstCombine.cpp` 1280-1320 (foldConsecutiveLoads guarded by both LI->isSimple()), 1470-1550 (foldConsecutiveStores - matchPartStore guarded by `Store->isSimple()`).
  - `llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp` 1100-1150, 1796-2200 (all paths guard with isVolatile or isSimple).
  - `llvm/lib/Transforms/Scalar/LoopIdiomRecognize.cpp` 460-700 (isVolatile/isUnordered guards), 1170 (isVolatile=false hardcoded but precondition is unordered loop).
  - `llvm/lib/Transforms/Scalar/GVN.cpp` 1573-1605 (eliminatePartiallyRedundantLoad propagates volatile + ordering + syncscope).
  - `llvm/lib/Transforms/Scalar/JumpThreading.cpp` 1395-1420 (`new LoadInst` hardcodes volatile=false but guard at 1224 is `LoadI->isUnordered()` which excludes volatile and propagates ordering/syncscope).
  - `llvm/lib/Transforms/Scalar/LoopLoadElimination.cpp` 440-470 (`new LoadInst(...,false,...)` for preheader init; LAA upstream rejects atomic, verified by test).
  - `llvm/lib/Transforms/IPO/ArgumentPromotion.cpp` 230-260, 360-405 (guarded by `isSimple()` and `!isVolatile()` assert).
  - `llvm/lib/Transforms/IPO/GlobalOpt.cpp` 1280-1310 (TryToShrinkGlobalToBoolean; analyzeGlobal already guards with `LI->isSimple()`/`SI->isSimple()`).
  - `llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp` (`isSimple()` guards throughout - lines 1759, 2675, 7729, 8147, 9653, 9817, 10894, 13266, 15182, 25180, 27595).
  - `llvm/lib/Transforms/Vectorize/LoopVectorizationLegality.cpp` line 1775 (`SI->isSimple()` guard).
  - `llvm/lib/Transforms/Vectorize/LoadStoreVectorizer.cpp` line 1744 (`!LI->isSimple()|| !SI->isSimple()` guard).
  - `llvm/lib/CodeGen/InterleavedAccessPass.cpp` 506-555 (lowerInterleavedStore guarded by `SI->isSimple()`).
  - `llvm/lib/CodeGen/InterleavedLoadCombinePass.cpp` 873-880 (LI->isVolatile + LI->isAtomic both checked).
  - `llvm/lib/Transforms/Utils/LowerMemIntrinsics.cpp` - propagates SrcIsVolatile/DstIsVolatile throughout.
  - `llvm/lib/Transforms/Scalar/LowerMatrixIntrinsics.cpp` - propagates IsVolatile.
  - `llvm/lib/Transforms/Scalar/Scalarizer.cpp` 1220-1270 (gated by `LI.isSimple()`/`SI.isSimple()`).
  - `llvm/lib/Transforms/Scalar/LoopInterchange.cpp` 187-191 (Ld->isSimple() / St->isSimple()).
  - `llvm/lib/Transforms/Scalar/ConstraintElimination.cpp` 1171/1175 (only reads facts; no IR-create).
  - `llvm/lib/Transforms/Scalar/GVNSink.cpp` 350-365 (rejects atomic).
  - `llvm/lib/CodeGen/AtomicExpandPass.cpp` 564-820, 1200-1450, 1769-2280 (propagates volatile throughout via setVolatile after CreateLoad/Store; `insertRMWLLSCLoop` at 1357 does not take isVolatile but only called for partword RMW with strict ordering).
  - `llvm/lib/CodeGen/StackProtector.cpp` 559, 683, 722 (`true` for volatile hardcoded).
  - `llvm/lib/CodeGen/SafeStack.cpp` (internal alloca/stack-ptr mgmt; not user-visible volatile).
  - `llvm/lib/CodeGen/CodeGenPrepare.cpp` 8568-8665 splitMergedValStore: checks volatile but not atomic. Confirmed via `opt -passes=codegenprepare` that an `i64 store atomic` with `or(shl(hi,32), lo)` pattern crashes opt (separate issue) and is not split silently. NOT counted as silent-miscompile candidate.
  - `llvm/lib/CodeGen/SjLjEHPrepare.cpp` 143-243 (`true` for volatile hardcoded).
  - `llvm/lib/CodeGen/WinEHPrepare.cpp` 1275-1430 (internal spill slot).
  - `llvm/lib/CodeGen/ShadowStackGCLowering.cpp` (internal GC bookkeeping).
  - `llvm/lib/Transforms/Coroutines/CoroFrame.cpp` (internal frame spill/load).
  - `llvm/lib/Target/X86/X86InterleavedAccess.cpp` 218, 794: lowerInterleavedStore in target hook; caller `InterleavedAccessImpl::lowerInterleavedStore` gates by `SI->isSimple()`.
  - `llvm/lib/Target/X86/X86LowerAMXType.cpp`, `X86InstCombineIntrinsic.cpp` (AMX/intrinsic-specific, internal-alloca temps).
  - `llvm/lib/Target/ARM/ARMParallelDSP.cpp` 352, 774 (`Ld->isSimple()` guard).
  - `llvm/lib/Transforms/IPO/AttributorAttributes.cpp` 4204 isDeadStore checks isVolatile but not isAtomic; verified via repro that captured-alloca atomic stores are preserved (mem-effects analysis intervenes). Did not file as a candidate.
- Patterns ruled out (with concrete tests):
  - `opt -passes=instcombine,gvn,early-cse,licm,slp-vectorizer,loop-vectorize` on `load atomic i32 unordered` through select, store-load forwarding, GEP: all preserve the atomic marker.
  - `opt -passes=aggressive-instcombine` on `load volatile` consecutive-pair fold: does not fold (isSimple guard).
  - `opt -passes=loop-load-elim` on `load atomic unordered` in a loop: does not fire (LAA rejects).
  - `opt -passes=sroa` on `load volatile` from a sliced alloca: `volatile` IS preserved (only `atomic` is lost; SROA pre-filter bails on isVolatile).
- Potential bugs filed:
  - `candidates/w61-sroa-drops-atomic-on-promoted-loadstore.md` — SROA's vector-promotion / integer-widening slice rewriter (`AllocaSliceRewriter::visitLoadInst` / `visitStoreInst` callsites at SROA.cpp:3164-3168, 3198-3206, 3354-3372) checks `if (LI.isVolatile()) NewLI->setAtomic(...)` -- the predicate is the wrong one. Atomic-only loads/stores (e.g. `load atomic i32 ... unordered`) pass the pre-filter (which only checks `isVolatile()`) and end up replaced by plain non-atomic load/store. Confirmed: `opt -passes=sroa` on a `<2 x i32>`-alloca with `load atomic i32 unordered` produces a plain `extractelement` chain with the `atomic` qualifier silently dropped. Also reproduced on partial-alloca atomic store and integer-widening path.
  - `candidates/w61-simplifycfg-mergeCondStores-drops-atomic.md` — SimplifyCFG `mergeConditionalStoresEdge` (line 4408) creates the merged store via plain `QB.CreateStore(QPHI, Address)` with no `setAtomic` afterwards. The filter at line 4275 is `isUnordered()` which accepts both `NotAtomic` and `Unordered` atomic stores. Confirmed: `opt -passes='simplifycfg<>'` on two `store atomic i32 ... unordered` in two predecessor blocks merges them into a single non-atomic `store i32`, dropping the atomic qualifier.

Other observations (no candidate filed):
- The `LowerAtomic` pass `expandPartwordAtomicRMW` (AtomicExpandPass.cpp:1066) calls `insertRMWLLSCLoop` (1357) which does not thread `isVolatile()` into `emitLoadLinked` / `emitStoreConditional`. This is a target-hook concern, not the IR-level volatile-drop family.
- `CodeGenPrepare splitMergedValStore` (8568) does not gate on `isAtomic()`. A `store atomic` going through this path crashes (rather than silently miscompiles); already partially covered by #012 for volatile.
- The comment in SROA.cpp:1722-1740 explicitly says "atomic semantics do not have any meaning for a local alloca" -- but that reasoning only holds when the alloca is not captured. Did not file as a separate bug because the captured-alloca case requires a more elaborate reproducer and SROA's `isSafeSelectToSpeculate` only opts in non-volatile loads anyway.

## worker-65 2026-05-21
- Hunt: x86 runtime miscompiles in IndVarSimplify / SimplifyIndVar / ScalarEvolution / LoopStrengthReduce.
- Approach: write small IR loops, run `opt -passes=indvars -S` (also `-O3` for unroll fusion), compile both opt and llc -O0 with `llc -O2` (assembled via in-tree clang because system as does not know `.prefalign`), drive with a C runner that mirrors loop semantics carefully (sum-of-phi-value vs sum.next pitfall noted).
- IR patterns exercised end-to-end (all matched runtime):
  - Two-IV closing-rate {iv += nuw 1, rhs -= nsw 1, iv ult rhs}: SCEV closed form `umax(rhs-1,1)*…` correct for `rhs_start ∈ {1..1000}` (corners that should expose bug 088 are UB-infinite, not observable).
  - Negative-stride `iv = n; iv -= nsw 1; iv sgt 0` — closed form `n*(n-1)/2` verified for n up to 65536.
  - `eliminateTrunc` shape `i64 iv → trunc i32 ult trunc(n)`: SCEV uses low-32 of n; verified for n with high bits set (`0x100000000…0x200000005`).
  - LFTR with non-unit stride 3 (`add nsw iv,3; iv slt n`): closed form correct for n up to 10^6.
  - Two-IV multiplicative body `sum += i*j` with i++/j--: full closed form via -O3; correct for n up to 10^5.
  - Geometric `shl nuw nsw iv,1` exit: indvars doesn't touch (no AddRec model), no regression.
  - Post-inc `ne` exit `iv += nsw 1; iv.next ne n`: closed form `n-s`; correct for all well-defined inputs.
  - Trunc-based exit `(i32)iv.next != 0` with i64 IV stride 3: SCEV returns `12884901888 = 3*2^32` (lcm correct).
- File: llvm/lib/Transforms/Utils/SimplifyIndVar.cpp:489-596 (`eliminateTrunc`) — read; lines 562-564 require both operands non-negative for `CanUseZExt` on signed non-equality predicates. Soundness depends on `SE->isKnownNonNegative` which itself uses signed-range — could fail open on a constant whose SCEV is `unknown` but proven non-negative by other means; not exploitable here.
- File: llvm/lib/Transforms/Utils/SimplifyIndVar.cpp:1609-1652 (`widenLoopCompare`) — line 1629 `Cmp->hasSameSign() ? IsSigned : Cmp->isSigned()` is a `samesign`-attribute aware selection; line 1646 special-case `DU.NeverNegative && isa<SExtInst>(Op) && !Cmp->isSigned()` flips to sext widening. The flip is safe because under non-negativity of LHS the sext and zext of LHS coincide; sext of RHS preserves the original unsigned comparison only if RHS in narrow type was also non-negative-equivalent-as-bits. Reviewed source, no obvious soundness gap.
- File: llvm/lib/Transforms/Scalar/IndVarSimplify.cpp:1060-1196 (`linearFunctionTestReplace`) — read; flag-dropping at 1099-1117 correctly narrows nuw/nsw to AR-proven flags, but only on the increment instruction (`IncVar`), not on intermediate uses; comment at 1107-1110 acknowledges first-iteration dynamic-deadness gap as a TODO.
- File: llvm/lib/Analysis/ScalarEvolution.cpp:2416-2490 (`willNotOverflow`) — read; context-based fallback (`isKnownPredicateAt` with `CtxI`) bails for `SINT_MIN` constant inversion (line 2470); this is the SCEV side of the (C) gap noted in candidate w44.
- Potential bugs filed:
  - candidates/w65-indvars-scev-fuzz-no-runtime-misc.md — *no* new candidate; documents 8 verified-correct patterns + notes unexercised areas (LSR runtime, `samesign` interactions, multi-exit loops, applyLoopGuards/assume-bundle).
- New confirmed bugs filed: 0.

## worker-64 2026-05-21
- Hunt: x86 LLVM miscompiles involving undef/poison/freeze propagation.
- Approach: hand-crafted IR around known-hot LangRef edges (nnan/ninf produce poison, freeze of poison, select with undef cond + poison arm, partial-undef vector divisor, vector demanded-elements lane→poison, fcmp ord/uno + undef + nnan, fdiv 0/0 / sqrt(-1) / log(-1) / fma(0,inf,0) under nnan, fadd(inf,-inf), ldexp NaN, x86 psra/psrl/pmulhuw with m_One and partial undef, sub nuw 0,x, neg INT_MIN nsw, fcmp nnan oeq/ord/uno with NaN constant, fshl/fshr shift=0 vs bitwidth, freeze undef as branch condition, GVN/SimplifyCFG merging two freezes of same value, phi of two undef with self-arith). Each candidate run through `opt -passes=instcombine -S` and cross-checked against the LangRef refinement rule (defined→poison ⇒ miscompile; poison→defined or undef→defined ⇒ valid refinement).
- File: llvm/lib/Analysis/ConstantFolding.cpp:1573-1610 (`ConstantFoldFPInstOperands`) — FMF check is limited to nsz/algebraic and a NaN-payload bail under `!AllowNonDeterministic`; nnan/ninf never inspected.
- File: llvm/lib/Analysis/ConstantFolding.cpp:2263-2284 (`ConstantFoldFP`) for unary FP intrinsics (sqrt, log, log2, exp, exp2, sin, cos, tan, atan, fma, fmuladd, pow, rint, canonicalize, ...) — entirely FMF-agnostic.
- File: llvm/lib/Analysis/InstructionSimplify.cpp:566-590 (`foldOrCommuteConstant`) — routes FP binops through `ConstantFoldFPInstOperands(..., /*AllowNonDeterministic=*/default true)`, so even the NaN-payload bail never fires from InstSimplify; FMF-aware folds at lines 6150-6197 (`simplifyFDivInst`) only fire after the constant fold and only catch `nnan&ninf X / [-]0.0`.
- File: llvm/lib/Analysis/InstructionSimplify.cpp:4159-4220 (`simplifyFCmpInst`) — constant fold via `ConstantFoldCompareInstOperands` at line 4167 dominates and never sees FMF; the FMF-aware paths at 4209-4218 only run for non-constant operands.
- File: llvm/lib/Analysis/InstructionSimplify.cpp:5037-5062 (`simplifySelectInst`) — `select undef, X, Y` returns Y when Y is a Constant else X; produces a single PoisonValue for `select undef, %x, poison`, which is a valid refinement (poison ∈ {X, poison}) but worth noting.
- File: llvm/lib/Transforms/InstCombine/InstCombineNegator.cpp:120-130, 273-288 — `-(undef)` and SDiv negation already guard against undef/poison; no defect.
- File: llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp grep for m_One/m_Zero — partial-undef vector divisors fold to poison; valid because a 0 in any divisor lane is UB for the whole vector div.
- Patterns ruled out (refinements but not miscompiles):
  - `select undef, X, Y` choosing one specific arm (including poison-arm) is valid refinement.
  - `sub nuw 0, x → 0` for x ∈ unknown is valid (poison refined to 0).
  - `add nsw 2147483647, 1 → INT_MIN` and similar overflow folds refine poison to wrap-around value.
  - `freeze i1 undef → false` and `freeze i32 undef → 0` are valid refinements of "arbitrary value".
  - `srem x, splat 1 → 0` and `sdiv vec %x, <1, undef, 1, 1> → poison` are valid because any zero divisor lane is UB.
  - `freeze i32 (sdiv 1, 0) → 0` and `chain_poison() → -2147483647` are refinements of `freeze poison`.
  - `xor undef, undef → 0` is a valid refinement (two undefs can be any value, including equal).
  - `phi [undef, t], [undef, e]; and p,1; icmp eq → true` chooses undef=0, refinement of undef.
  - `fcmp ord NaN, 1.0 → false` (no FMF) is correct per LangRef.
  - `pmulhuw <1, undef, ...>` covered by w04 already.
  - `psra big_shift` correctly clamps to bitwidth-1 per x86 semantics.
- Potential bugs filed:
  - (removed) two `nnan`/`ninf` constfold candidates (binop + fcmp) that fold to a NaN/Inf/bool constant when the LangRef contract says the result is `poison`. Dismissed: LangRef explicitly permits replacing a poison value with any concrete value, so folding poison → finite constant is a sound refinement, not a miscompile.
- New confirmed bugs filed: 0.

## worker-91 2026-05-21
- File: llvm/lib/CodeGen/SelectionDAG/LegalizeDAG.cpp (expandLdexp, expandFrexp, ExpandFPLibCall, ConvertNodeToLibcall switch arms for FLDEXP/FPOWI/FFREXP/FMODF, FP_TO_INT_SAT clamp).
- File: llvm/lib/CodeGen/SelectionDAG/LegalizeFloatTypes.cpp (SoftenFloatRes_BF16_TO_FP, SoftenFloatRes_FCANONICALIZE, SoftenFloatRes_ExpOp, SoftenFloatRes_FFREXP, SoftPromoteHalfRes_UnaryOp).
- File: llvm/lib/CodeGen/SelectionDAG/LegalizeIntegerTypes.cpp (PromoteIntRes_BITCAST, PromoteIntOp_ZERO_EXTEND nneg path, ExpandIntRes_Logical disjoint propagation, shift-amount expansion FIXMEs).
- File: llvm/lib/CodeGen/SelectionDAG/LegalizeVectorOps.cpp (ExpandFNEG XOR-sign-mask, ExpandUINT_TO_FP strict variant, VP_SETCC mask/evl drop, ExpandSETCC).
- File: llvm/lib/CodeGen/SelectionDAG/LegalizeVectorTypes.cpp (ScalarizeVecRes_INSERT_VECTOR_ELT TRUNCATE-on-float FIXME, SplitVecRes_INSERT_VECTOR_ELT byte-rounding, WidenVecRes_OverflowOp poison padding, ScalarizeVecRes_SETCC drops flags).
- File: llvm/lib/CodeGen/SelectionDAG/TargetLowering.cpp (SimplifyDemandedBits SHL/SRL/FSHL/FSHR/INSERT_VECTOR_ELT/INSERT_SUBVECTOR/EXTRACT_VECTOR_ELT, ShrinkDemandedConstant OR-disjoint flag, ShrinkDemandedOp, expandFP_ROUND bf16 round-inexact-to-odd, expandFP_TO_INT_SAT NaN-handling, softenSetCCOperands strict-signaling, expandMultipleResultFPLibCall).
- File: llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp (simplifyShift, simplifyFPBinop, simplifySelect).
- Patterns ruled out:
  - SoftenFloatRes_BF16_TO_FP strict-chain FIXME: real source-confirmed but not user-reachable on x86 (f128 is legal, bf16 strict-fp goes via PromoteFloat / SoftPromoteHalf paths emitting __extendbfsf2 + __extendsftf2 chain).
  - SoftenFloatRes_FCANONICALIZE chain discard: source comment confirms it is intentional and the STRICT_FMUL has NoFPExcept set, so safe.
  - DAGCombiner simplifyDivRem X/X -> 1, X%X -> 0: X=0 case is refinement of undef/poison, legal.
  - simplifyShift `shift i1 X, Y -> X`: refinement of poison for Y!=0, legal.
  - TargetLowering INSERT_VECTOR_ELT/INSERT_SUBVECTOR/EXTRACT_VECTOR_ELT simplification cases reviewed without finding new bugs.
  - ExpandFNEG vector via XOR(SignMask): IEEE-754 permits FNEG to not quiet sNaN (sign-bit op), so not a bug (the related `fdiv X,-1.0 → xorps` pattern that elides FDIV semantics is a known sNaN-quieting loss that we no longer pursue).
- Potential bugs filed:
  - candidates/w91-strict-ldexp-i64-libcall-silent-truncation.md — STRICT variant of #011: `llvm.experimental.constrained.ldexp.f64.i64` lowers to `ldexp@PLT` with i64 in %rdi (low 32 bits read as int, high silently dropped, no diagnostic); root cause: `expandLdexp` bails on STRICT_FLDEXP (LegalizeDAG.cpp:2572 TODO) and `ConvertNodeToLibcall` for FLDEXP/STRICT_FLDEXP (5031-5035) lacks the FPOWI-style sizeof(int) guard.
  - candidates/w91-frexp-i64-libcall-stack-slot-overrun.md — `llvm.frexp.f64.i64` allocates an i64 stack slot for the exponent output, calls `frexp(double, int*)` which writes only 4 bytes, then loads 8 bytes back; high 32 bits leak the caller's stale `%rax` (via the slot-establishing `pushq %rax`). Root cause: `TargetLowering::expandMultipleResultFPLibCall` (TargetLowering.cpp:13245+) sizes both the temp slot and the read-back load by `Node->getValueType(ResNo)` without an `IntSize` width check.

## worker-70 2026-05-21
- File: llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp — broad SLP hunt focused on FP/integer reductions, alt-shuffle, MinBWs narrowing, copyable elements, propagateIRFlags, non-pow-of-2 vectors, store-chain rewrite (d5ad8116f).
- Approach: Diverse runtime-equivalence fuzzer (~1500 random seeds across integer arith, FP, cmp/select, MinBWs, non-pow-of-2 widths) comparing scalar IR vs SLP-vectorized IR linked together and run on random inputs with NaN/Inf/denormal coverage.
- Hand-tested patterns:
  - FAdd/FSub chains without fast-math (correctly NOT vectorized when ordered)
  - smax/smin reductions with mixed signs (correctly vectorize to vector.reduce.smax/smin)
  - add nsw/nuw widening (flags correctly dropped via andIRFlags)
  - alt-shuffle fadd/fsub, shl/lshr (correctly use shufflevector to pick lanes)
  - canConvertToFMA: multi-use FMul correctly not fused
  - Copyable elements: passthrough lanes get correct add-0/sub-0/shl-0 identities
  - Logical AND/OR short-circuit: SLP correctly adds freeze for poison-blocking
  - MinBWs narrowing zext slt to i16 (correct because zext non-negative)
  - icmp samesign on lane 0 only: correctly dropped via andIRFlags
  - i8 -> i32 zext multiply chain: correctly narrowed
- Patterns ruled out:
  - propagateIRFlags(IncludeWrapFlags=false) path properly removes nsw/nuw from vectorized arith
  - copyIRFlags wrap flag plumbing for trunc looks safe in SLP context (trunc never propagated by SLP CreateBinOp path)
  - HorizontalReduction::createOp drops wrap flags via IncludeWrapFlags=false
  - Volatile load between stores blocks SLP store-vectorization correctly
- Potential bugs filed: NONE — no runtime miscompile found in default pipeline within time budget.

## worker-63b 2026-05-21
- Hunt focus: less-obvious volatile/atomic-strip passes + integer-arith miscompiles in directories user flagged (JumpThreading, LoopUtils, LoopIdiomRecognize, SeparateConstOffsetFromGEP, PlaceSafepoints, GlobalOpt, ScalarizeMaskedMemIntrin, InferAlignment, SafeStack, InductiveRangeCheckElimination, InstCombine, IndVarSimplify, LoopVectorize, VectorCombine, LowerMatrixIntrinsics) plus GISel `add/sub i128/i256` family (already-known bug 110/111 sibling expansions: ascertained `add i128/i256`, `sub i128/i256/i192/i96/i65`, `neg/inc/dec i128`, `usub.with.overflow.i128` all share the same `setb %sX; cmpb $1, %sX; adcq/sbbq` root cause — single fix would close them all, no separate filings).
- Reviewed without finding new bugs:
  - JumpThreading simplifyPartiallyRedundantLoad: gated on `LoadI->isUnordered()` (rejects volatile and ordered atomic).
  - LoopLoadElimination propagateStoredValueToLoadUsers: passes `isVolatile=false` to new pre-header LoadInst BUT upstream `LoopAccessInfo` already rejects loops with non-simple loads/stores (`LoopAccessAnalysis.cpp:2635, 2659`), so unreachable on volatile/atomic.
  - LoopIdiomRecognize processLoopMemoryCopy/MemSet (lines 461, 819, 882, 1425): correctly checks `isVolatile()` / `isUnordered()` before forming memcpy/memset; for atomic stores it emits ElementUnorderedAtomicMemCpy via `getAtomicMemIntrinsicMaxElementSize`.
  - InferAlignment (entire file): only adjusts alignment metadata (a hint upgrade); not a value change. Safe for volatile/atomic.
  - SeparateConstOffsetFromGEP: works on GEPs only; never touches load/store volatile/atomic state.
  - PlaceSafepoints/GlobalOpt (TryToShrinkGlobalToBoolean): GlobalStatus.cpp:95/104 rejects globals with volatile loads/stores; GlobalOpt `processInternalGlobal` further requires `GS.Ordering == NotAtomic` for the shrink-to-bool and stored-once paths. Safe.
  - ScalarizeMaskedMemIntrin: masked intrinsics carry no volatile flag in IR; CreateAlignedLoad/Store on resulting scalar pieces is correct.
  - LowerAtomicPass: only rewrites atomic ordering via `setAtomic(NotAtomic)`; `LowerAtomic.cpp` `lowerAtomicRMWInst`/`lowerAtomicCmpXchgInst` already filed as bug 111.
  - SafeStack: thread-local stack-pointer accesses, no user-controlled volatile semantics.
  - SimplifyCFG sink/hoist common: `hasSameSpecialState` checks volatile equality; sinking N branch-conditional volatile loads collapses to 1 load that executes exactly once per dynamic path — preserves dynamic volatile-access count.
  - SCCP/IPSCCP visitLoadInst (SCCPSolver.cpp:1893): only folds loads of `constant` globals, where ordering is irrelevant. Safe.
  - MergedLoadStoreMotion: line 331 checks `S0->isSimple()`; safe.
  - DSE isRemovable (DeadStoreElimination.cpp:1452-1471): correctly checks `isUnordered()` for stores and `!isVolatile()` for memintrins.
  - GVN eliminatePartiallyRedundantLoad (GVN.cpp:1573): propagates `Load->isVolatile()`, `Load->getOrdering()`, `Load->getSyncScopeID()` to new LoadInst.
  - ArgumentPromotion HandleEndUser (ArgumentPromotion.cpp:557): `I->isSimple()` gate.
  - ConstraintElimination only uses load/store as facts; doesn't mutate them.
  - FunctionSpecialization getPromotableAlloca: stores to local alloca; atomic on alloca has no cross-thread meaning (alloca not escaped to other threads in spec context).
  - VectorCombine vectorizeLoadInsert / widenSubvectorLoad / shrinkLoadForShuffles / foldSingleElementStore: all use `Load->isSimple()` / `SI->isSimple()` gating. Safe.
  - InstCombine select-folds, sext+add, distribute mul, freeze+nuw add, abs(abs), avgfloor/ceil, mod-pow-of-2: hand-verified ~25 patterns for correctness via Alive2-style reasoning. None miscompile.
  - Recent commit `c497efb82` (InstCombine logical and/or trunc-nuw->i1 fold): proof linked, `isGuaranteedNotToBePoison(A) && KnownBits(A).getMaxValue() == 1` predicate is correct.
  - Recent commit `f63b8ee1e` (X86 combineSelect with non-i1 cond): fixed by gating `Cond.isVector()`; other call sites of `IsNOT` (lines 49586, 51847, 56860, 56956, 58094) are all vector-typed paths.
- Potential bugs filed:
  - candidates/w63b-lower-matrix-fuseFlatten-drops-volatile.md — `LowerMatrixIntrinsics.cpp:1723` matmul "dot-product flatten" path matches a `matrix.column.major.load` and emits `Builder.CreateLoad(Op->getType(), Arg)` without forwarding the intrinsic's `i1 immarg %isVolatile` (arg index 2). When fusion picks the flatten lowering (stride=1 via `CanBeFlattened` `m_One()`), a `volatile` matrix load is silently rewritten to a non-volatile load and freely DCE'd/coalesced by later passes.
  - candidates/w63b-vectorcombine-scalarizeLoadExtract-strips-atomic.md — `VectorCombine.cpp:2015` entry to `scalarizeLoad` only rejects `isVolatile()`; an `atomic` (unordered/monotonic) `<N x T>` load passes through to `scalarizeLoadExtract` (line 2130) or `scalarizeLoadBitcast` (line 2202), where it is broken into N plain non-atomic scalar `Builder.CreateLoad(...)` loads. The no-torn-read guarantee is silently lost. Fix: tighten the gate to `LI->isSimple()`. Reproduces with `-O3` (vector-combine is in the default pipeline).

## worker-66 2026-05-21

Hunt: passes that drop volatile/atomic when creating new load/store, especially
AtomicExpandPass, GlobalOpt, LowerMatrixIntrinsics, SafeStack, FunctionAttrs,
ArgumentPromotion, InferAddressSpaces, RewriteStatepointsForGC, SCCPSolver,
CGP, Sink, SimplifyLibCalls, LoopIdiomRecognize, MergedLoadStoreMotion,
LICM, LoopVectorize, JumpThreading, NewGVN, GVNHoist, InstCombine helpers,
SLPVectorizer, Scalarizer, EarlyCSE, VectorCombine, MemorySanitizer,
ConstantHoisting, LoopRotation, TailRecursionElimination, Reassociate, SCCP,
WinEHPrepare, AttributorAttributes, OpenMPOpt, CallSiteSplitting,
ExpandIRInsts, ExpandVectorPredication, JumpTableToSwitch, AggressiveInstCombine.

Files actively read:
- llvm/lib/CodeGen/AtomicExpandPass.cpp (full pass)
- llvm/lib/Transforms/Utils/LowerAtomic.cpp (already w57)
- llvm/lib/Transforms/Scalar/LowerMatrixIntrinsics.cpp (FlattenArg path)
- llvm/lib/Transforms/Scalar/InferAddressSpaces.cpp (memintrinsic guard)
- llvm/lib/Transforms/IPO/FunctionAttrs.cpp (memory effects)
- llvm/lib/Transforms/IPO/GlobalOpt.cpp (TryToShrinkGlobalToBoolean -
  already w57 cleared via GlobalStatus guard)
- llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp
- llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp
  (SimplifyAnyMemTransfer/Set, masked.scatter)
- llvm/lib/Transforms/InstCombine/InstCombinePHI.cpp
  (foldPHIArgLoadIntoPHI)
- llvm/lib/Transforms/Scalar/Sink.cpp
- llvm/lib/Transforms/Scalar/MergedLoadStoreMotion.cpp (already w57)
- llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp (processStore,
  processStoreOfLoad, tryMergingIntoMemset)
- llvm/lib/Transforms/Scalar/JumpThreading.cpp (1406 PRE load is isUnordered-guarded)
- llvm/lib/Transforms/Scalar/GVN.cpp (1573 NewLoad preserves vol+ordering+SSID)
- llvm/lib/Transforms/Scalar/NewGVN.cpp (1558 isSimple guard)
- llvm/lib/Transforms/Scalar/GVNHoist.cpp (LoadInfo/StoreInfo isSimple-guarded)
- llvm/lib/Transforms/Scalar/LICM.cpp (promotion tracks SawUnorderedAtomic)
- llvm/lib/Transforms/Vectorize/LoadStoreVectorizer.cpp (line 1744 isSimple)
- llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp
- llvm/lib/Transforms/Vectorize/VectorCombine.cpp (1953/1971 isSimple)
- llvm/lib/Transforms/IPO/ArgumentPromotion.cpp (402/558 guarded)
- llvm/lib/Transforms/IPO/AttributorAttributes.cpp (privatize paths use Alloca)
- llvm/lib/Transforms/Utils/PromoteMemoryToRegister.cpp
- llvm/lib/Transforms/Utils/CodeExtractor.cpp (alloca-based, OK)
- llvm/lib/Transforms/Utils/SCCPSolver.cpp (visitLoadInst gated by isVolatile)
- llvm/lib/Transforms/Utils/LoopUtils.cpp (collectInstructionsToDuplicate
  rejects volatile/atomic load)
- llvm/lib/CodeGen/CodeGenPrepare.cpp (CGP splitMergedValStore already filed)
- llvm/lib/CodeGen/SafeStack.cpp (only touches alloca/byval, safe)
- llvm/lib/CodeGen/WinEHPrepare.cpp (alloca spill slots)
- llvm/lib/CodeGen/GCRootLowering.cpp (gcread/gcwrite have no IR vol/atomic)
- llvm/lib/CodeGen/ExpandVectorPredication.cpp (vp_load/vp_store have no vol)
- llvm/lib/Transforms/Scalar/LoopIdiomRecognize.cpp (461/513/548/882 guarded)

Patterns ruled out / verified safe:
- InferAddressSpaces handleMemIntrinsicPtrUse path: guarded by
  `!MI->isVolatile()` at line 1441 before calling the helper that hardcodes
  isVolatile=false.
- GlobalOpt TryToShrinkGlobalToBoolean / OptimizeGlobalAddressOfAllocation
  preserves SI->getOrdering() and SI->getSyncScopeID() (lines 1287, 1305,
  1299); GlobalStatus already rejects any volatile use of the GV.
- RewriteStatepointsForGC's `new LoadInst`/`new StoreInst` calls all target
  freshly-created allocas (PromotableAllocas / Spill slots), so dropped
  volatile is on private memory.
- InstCombinePHI foldPHIArgLoadIntoPHI: line 708 `!FirstLI->isSimple()`
  bails on volatile or atomic loads before the new LoadInst with
  /*IsVolatile=*/false.
- InstCombineLoadStoreAlloca unpackLoadToAggregate / unpackStoreToAggregate
  both gated by isSimple at the entry.
- InstCombineCalls SimplifyAnyMemTransfer: propagates volatile via
  L->setVolatile(MT->isVolatile()) / S->setVolatile(MT->isVolatile()) at
  207-208.
- InstCombineCalls masked.scatter (lines 401, 418): the intrinsic doesn't
  have an isVolatile arg so dropping is not a regression.
- Sink: `IsAcceptableTarget` line 87 bails on `mayReadFromMemory()` so a
  volatile load cannot be sunk across a critical edge anyway.
- ExpandVectorPredication: vp_load/vp_store intrinsics have no isVolatile arg
  so the hardcoded /*IsVolatile=*/false at 438, 450 is correct.
- LoopUtils collectInstructionsToDuplicate (line 2346) bails on volatile or
  atomic load.
- MergedLoadStoreMotion mergeStores (line 331) bails on `!S0->isSimple()`.
- LoopIdiomRecognize: line 461 bails on volatile store before
  CreateMemSet with isVolatile=false (1170); processLoopMemoryCopy
  (1425-1492) correctly switches to CreateElementUnorderedAtomicMemCpy
  when IsAtomic, and the non-atomic branch's CreateMemCpy with false is
  reached only after isVolatile guards in isLegalStore/isLegalLoad.
- LoopVectorize / SLPVectorizer: LoopAccessAnalysis line 2635/2659 bails on
  non-simple load/store (except parallel_accesses annotation).
- VectorCombine foldSingleElementStore (1953 / 1971), canWidenLoad (220):
  all isSimple-guarded.
- AttributorAttributes privatize paths (7620, 7627, 7630, 7653, 7662, 7667):
  target newly-created allocas representing privatized args.
- WinEHPrepare loads/stores (1280, 1421, 1428): all target alloca-based
  spill slots for EH PHI flow.
- AtomicExpandPass expandAtomicLoadToCmpXchg: dropped volatile + syncscope -
  FILED below.
- AtomicExpandPass expandAtomicStoreToXChg: dropped volatile + syncscope -
  FILED below.
- LowerMatrixIntrinsics FlattenArg (lines 1718-1726): drops volatile when
  the matrix dot-product fused path replaces a matrix.column.major.load
  intrinsic that has i1 true isVolatile arg with a plain CreateLoad -
  FILED below.
- AtomicExpandPass expandAtomicLoadToLL (line 652-666): doesn't pass volatile
  or syncscope to emitLoadLinked; target-specific lowering. Not exercised on
  x86 path so not filed.
- AtomicExpandPass insertRMWLLSCLoop: doesn't take volatile/SSID; LLSC path
  isn't used on x86.

Candidates filed:
- candidates/w66-atomic-expand-load-to-cmpxchg-drops-volatile-syncscope.md
  (i128 `load atomic volatile syncscope("singlethread")` on
  `x86_64 -mattr=+cx16` becomes plain `cmpxchg ... seq_cst seq_cst` with
  neither volatile nor singlethread)
- candidates/w66-atomic-expand-store-to-xchg-drops-volatile-syncscope.md
  (i128 `store atomic volatile syncscope("singlethread")` on
  `x86_64 -mattr=+cx16` becomes a seed `load i128` (non-atomic, non-volatile)
  followed by a `cmpxchg ... seq_cst seq_cst` with neither volatile nor
  singlethread)
- candidates/w66-lower-matrix-intrinsics-flatten-drops-volatile.md
  (matrix dot-product fused lowering replaces
  `llvm.matrix.column.major.load(... i1 true ...)` with plain `Builder.CreateLoad`,
  losing volatile)
