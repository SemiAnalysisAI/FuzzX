# X86 LLVM bug hunt

*Human-written portion*

The bugs here were found not by fuzzing, but by code inspection.  I simply
asked Claude to find bugs in LLVM, and gave it a little guidance along the way.

I wouldn't consider all of these to be serious bugs, and some I'd say aren't
bugs at all.  Others appear to be real miscompiles, such as
[#195](bugs/195-instcombine-ldexp-chain-integer-overflow/NOTES.md).

Everything below here is machine-generated.  Good luck.

------------

Goal: find ≥100 real bugs in the x86 path through the default LLVM pass pipeline.

**Status: 126 reproducible bugs (well past the 100 goal). 225 total catalog entries (~99 are source-confirmed only). 500 pending candidate notes in `candidates/` not yet promoted.**

Breakdown by repro kind:
- crash (4): #071, #218, #222, #227
- hang (1): #191
- runtime miscompile (3): #003 (GISel-only), #004, #013
- asm/asm-diff (12): #001, #005, #008, #009, #010, #011, #012, #014, #140, #240, #357, …
- mir-diff (19): #124, #125, #196, #198, #199, #208–#210, #213, #226, #231, #237, #238, #239, …
- opt-diff (~99): all others

Most reproducible bugs fall in: metadata loss (`!nontemporal`, `!invariant.load`, `!alias.scope`, `!range`, FMF, `samesign`, syncscope, `!unpredictable`, `!prof`), poison/refinement violations (#195/#206/#207/#252), and PGO corruptions (#232).


## Tools
- LLVM source: `../amdgpu/third_party/llvm-project/` (HEAD ≈ `0dd29960cd61` as of session start)
- opt/llc:    `../amdgpu/build/llvm-fuzzer/bin/{opt,llc,clang}`
- Default target triple: `x86_64-unknown-linux-gnu`

## Layout
- `bugs/NNN-short-name/` — one folder per confirmed bug, containing:
  - `NOTES.md`     — explanation of the bug, root cause if known
  - `repro.ll`     — minimal IR reproducer (or `.s`/`.c` if more natural)
  - `cmd.sh`       — exact command to reproduce
  - `expected.txt`, `actual.txt` (for miscompiles, when an executable repro exists)
- `candidates/`   — pre-triage notes from code-reading workers (not yet confirmed)
- `workers/`      — per-worker logs and ranges-already-explored, to avoid duplicate work

## Bug catalog
(Filled as bugs are confirmed.)

| # | Kind | Status |
|-----|------|--------|
| 001 | [001-volatile-atomicrmw-or-zero-drops-volatile](bugs/001-volatile-atomicrmw-or-zero-drops-volatile/) - volatile bit dropped from `atomicrmw or %p, 0` (idempotent RMW lowered as plain load) | PR [#199587](https://github.com/llvm/llvm-project/pull/199587) not merged |
| 003 | [003-gisel-uadde-inverted-carry](bugs/003-gisel-uadde-inverted-carry/) - `CMP r,1` for carry-in inverts CF; multi-word add/sub produce wrong upper word | PR [#199261](https://github.com/llvm/llvm-project/pull/199261) merged |
| 004 | [004-ldexp-avx512f-missing-cvtdq2ps](bugs/004-ldexp-avx512f-missing-cvtdq2ps/) - non-VLX AVX-512 path feeds int exp bits to `vscalefps` (missing `vcvtdq2ps`); `<4 x float>` `ldexp` returns x*1 | PR [#199263](https://github.com/llvm/llvm-project/pull/199263) merged |
| 005 | [005-fixupinsttuning-pslli-loses-changed](bugs/005-fixupinsttuning-pslli-loses-changed/) - `ProcessShiftLeftToAdd` mutates MI (`PSLLWri`→`PADDWrr`) but returns false; pass lies about preservation | PR [#199589](https://github.com/llvm/llvm-project/pull/199589) merged |
| 007 | [007-domain-reassignment-wrong-enclosed-key](bugs/007-domain-reassignment-wrong-enclosed-key/) - `EnclosedEdges[Reg] = ...` uses outer seed Reg instead of `CurReg`; only seed registered, latent duplicate-closure path | No PR |
| 008 | [008-returnthunks-missing-reti-lret-iret](bugs/008-returnthunks-missing-reti-lret-iret/) - matches only `RET32`/`RET64`; `RETI*`/`LRET*`/`IRET*` survive `fn_ret_thunk_extern` (Retbleed mitigation gap) | No PR |
| 009 | [009-ibt-wineh-funclet-missing-endbr](bugs/009-ibt-wineh-funclet-missing-endbr/) - catch / cleanup funclet entries get no `endbr64`; CET-IBT-enforcing host #CP-faults on every C++ exception | No PR |
| 010 | [010-lvi-cfi-missing-reti-lret-iret](bugs/010-lvi-cfi-missing-reti-lret-iret/) - only matches `RET64`; `RETI64`/`LRET*`/`IRET*` retain bare ret with no preceding lfence | No PR |
| 011 | [011-ldexp-i64-libcall-silent-truncation](bugs/011-ldexp-i64-libcall-silent-truncation/) - `llvm.ldexp.f64.i64` silently truncates exponent to int on libcall (POWI errors here, LDEXP didn't get the guard) | PR [#199177](https://github.com/llvm/llvm-project/pull/199177) not merged |
| 012 | [012-cgp-splitmergedvalstore-strips-atomic](bugs/012-cgp-splitmergedvalstore-strips-atomic/) - bail-out checks only `isVolatile()`; an atomic seq_cst i64 store is split into two non-atomic i32 stores | PR [#199592](https://github.com/llvm/llvm-project/pull/199592) merged |
| 013 | [013-instcombine-vector-reduce-mul-sext-i1-odd-lanes](bugs/013-instcombine-vector-reduce-mul-sext-i1-odd-lanes/) - `vector_reduce_mul(sext(<n x i1>))` folded to `zext(and-reduce(V))`; for odd n, all-true → +1 instead of -1 | PR [#199401](https://github.com/llvm/llvm-project/pull/199401) merged |
| 014 | [014-resetfpenv-mmo-flagged-as-store-on-load](bugs/014-resetfpenv-mmo-flagged-as-store-on-load/) - constant-pool FLDENVm load tagged `MOStore` (sister GET_FPENV_MEM correctly uses `MOLoad`) | No PR |
| 015 | [015-sfb-volatile-atomic-not-checked](bugs/015-sfb-volatile-atomic-not-checked/) - pass never checks `isVolatile()`/`isAtomic()` MMO flags; a volatile 16-byte XMM copy can be silently split | PR [#199698](https://github.com/llvm/llvm-project/pull/199698) merged |
| 016 | [016-vzeroupper-clobbersall-misses-ymm-zmm-16-31](bugs/016-vzeroupper-clobbersall-misses-ymm-zmm-16-31/) - `clobbersAllYmmAndZmmRegs` only iterates YMM/ZMM 0-15; upper bank invisible to dirty-state analysis | No PR |
| 017 | [017-atomicexpand-widenpartword-drops-volatile](bugs/017-atomicexpand-widenpartword-drops-volatile/) - `widenPartwordAtomicRMW` never calls `setVolatile(...)` on the widened RMW; sole outlier of all RMW expansion paths | PR [#199722](https://github.com/llvm/llvm-project/pull/199722) merged |
| 018 | [018-rvmarker-wrong-regmask-on-windows](bugs/018-rvmarker-wrong-regmask-on-windows/) - hard-codes `CallingConv::C` (SysV) preserved mask even on Win64; clobber set inconsistent with ABI | No PR |
| 019 | [019-frame-redzone-tcdelta-uint-underflow](bugs/019-frame-redzone-tcdelta-uint-underflow/) - `uint64_t = unsigned - int` for red-zone MinSize underflows when `TCReturnAddrDelta` is positive; absurd stack size | No PR |
| 020 | [020-matchvectoraddress-missing-wrapperrip](bugs/020-matchvectoraddress-missing-wrapperrip/) - handles `Wrapper` but not `WrapperRIP`; RIP-relative globals miss the disp32 fold for gather/scatter | No PR |
| 021 | [021-compress-evex-vpmov-physreg-use-operands-noop](bugs/021-compress-evex-vpmov-physreg-use-operands-noop/) - post-RA `MRI->use_operands(MaskReg)` on a physical kreg is a no-op; cross-BB live-out $k0 left uninitialized | No PR |
| 022 | [022-fpext-of-sitofp-drops-fmf](bugs/022-fpext-of-sitofp-drops-fmf/) - `fpext(sitofp x)` sunk via `CastInst::Create` drops FMF (sitofp/uitofp aren't FPMathOperators) | No PR |
| 023 | [023-foldfptoi-mask-asymmetric-fcposnormal](bugs/023-foldfptoi-mask-asymmetric-fcposnormal/) - uses `fcPosNormal` for fptoui but `fcNormal` for fptosi; negative-normal fptoui poison-refined to 0 | No PR |
| 024 | [024-foldshuffleofshuffles-poison-bool-cast](bugs/024-foldshuffleofshuffles-poison-bool-cast/) - `if (!NewX) return PoisonValue::get(Ty);` inside a `bool`-returning function; success path claimed without replaceValue | No PR |
| 025 | [025-machinesink-physdef-dead-not-zombie-checked](bugs/025-machinesink-physdef-dead-not-zombie-checked/) - EFLAGS guard trusts stale `dead` flag; doesn't scan source-block tail for late reader of the supposedly-dead physreg | No PR |
| 026 | [026-machinelicm-throwing-inline-asm-speculation](bugs/026-machinelicm-throwing-inline-asm-speculation/) - only checks dominator-of-exiting-blocks; ignores intra-loop throwing/INLINEASM-sideeffect — loads hoisted past faulting asm | No PR |
| 027 | [027-machinecse-implicit-def-positional-mismatch](bugs/027-machinecse-implicit-def-positional-mismatch/) - positional indexing into `CSMI->getOperand(i)` assumes MI/CSMI have identical operand layouts | No PR |
| 028 | [028-peephole-foldimmediate-no-tied-operand-check](bugs/028-peephole-foldimmediate-no-tied-operand-check/) - offers every explicit non-def operand to `foldImmediate` without `isRegTiedToDefOperand` check | No PR |
| 029 | [029-x87-adjustLiveRegs-stale-iterator](bugs/029-x87-adjustLiveRegs-stale-iterator/) - iterator mutated mid-loop in kill loop; caller reads stale state on subsequent iterations | No PR |
| 030 | [030-cfopt-cfi-unconditional](bugs/030-cfopt-cfi-unconditional/) - emits `.cfi_adjust_cfa_offset` whenever `!hasFP`, ignoring `needsDwarfCFI` — spurious CFI for `nounwind` funcs | No PR |
| 031 | [031-strict-fp-extend-chain-drop](bugs/031-strict-fp-extend-chain-drop/) - strict-fp f16→{f64,f80,fp128} on non-FP16 targets: outer STRICT_FP_EXTEND reuses original chain, dropping inner side effect | No PR |
| 032 | [032-fastTileConfig-cross-bb-shape-zero](bugs/032-fastTileConfig-cross-bb-shape-zero/) - per-BB processing misses tile def whose value reaches a PLDTILECFGV inserted in another BB; row/col stay zero | No PR |
| 033 | [033-cmov-conversion-eflags-liveness-misses-jmp](bugs/033-cmov-conversion-eflags-liveness-misses-jmp/) - `checkEFLAGSLive` returns false when LastCMOV has a kill marker even though later spliced instructions still read EFLAGS | No PR |
| 034 | [034-isKnownNeverNaN-fminnum-snan-or-incorrect](bugs/034-isKnownNeverNaN-fminnum-snan-or-incorrect/) - OR-logic for FMINNUM/FMAXNUM/*NUM unsound when SNaN=true; both-NaN case can return SNaN through unchanged | No PR |
| 035 | [035-fmul-neg1-fsub-negzero-snan-sign-flip](bugs/035-fmul-neg1-fsub-negzero-snan-sign-flip/) - `fmul X, -1.0` → `fsub -0.0, X` → `fneg X` without nnan; sNaN sign flipped, quieting lost (in-source FIXME) | No PR |
| 036 | [036-frame-swift-async-cfi-missing](bugs/036-frame-swift-async-cfi-missing/) - SwiftAsyncContext PUSH lacks CFA-offset CFI update; window between async PUSH and FP-establishing LEA has CFA-rule off by 8 | No PR |
| 037 | [037-pcmpestr-fold-load-missing-glue](bugs/037-pcmpestr-fold-load-missing-glue/) - glues EAX/EDX live-in CopyToRegs but doesn't include them in the CNode chain; folded load can reorder w.r.t. live-ins | No PR |
| 038 | [038-foldoffset-mul-imm-uint64-overflow](bugs/038-foldoffset-mul-imm-uint64-overflow/) - MUL-by-{3,5,9}→LEA shortcut does mixed-sign `int64*uint64` for the folded disp; fragile for negative `AddVal` / 32-bit wrap | No PR |
| 039 | [039-sextinreg-extload-multiuse](bugs/039-sextinreg-extload-multiuse/) - substitutes EXTLOAD with SEXTLOAD via `CombineTo(N0, ExtLoad, ExtLoad.getValue(1))` without `hasOneUse()` in OR-branch | No PR |
| 040 | [040-shl-of-shifted-logic-disjoint-propagation](bugs/040-shl-of-shifted-logic-disjoint-propagation/) - propagates `LogicOp->getFlags()` (incl. `disjoint`) verbatim to rewritten outer logic op without re-verifying | No PR |
| 041 | [041-sfb-blocker-no-volatile-check](bugs/041-sfb-blocker-no-volatile-check/) - companion to #015: blocking-store check also ignores volatile/atomic flags | PR [#199698](https://github.com/llvm/llvm-project/pull/199698) merged |
| 042 | [042-sfb-buildcopies-wrong-mmo-offset](bugs/042-sfb-buildcopies-wrong-mmo-offset/) - passes `LMMOffset` twice instead of `(LMMOffset, SMMOffset)`; harmless today but fragile | No PR |
| 043 | [043-compress-evex-vpmov-srcvec-clobber-kmov](bugs/043-compress-evex-vpmov-srcvec-clobber-kmov/) - kill-flag staleness when KMOV operand is overwritten in place | No PR |
| 045 | [045-winehstate-cleanup-skip-loses-hoist](bugs/045-winehstate-cleanup-skip-loses-hoist/) - state-store-emit loop skips entire cleanup-pad BBs; spurious -1 store emitted in non-cleanup successors | No PR |
| 046 | [046-cfopt-inline-asm-classify-side-effects](bugs/046-cfopt-inline-asm-classify-side-effects/) - INLINEASM that has side-effects but no `mayStore` is mis-classified; PUSH conversion reorders around it | No PR |
| 047 | [047-x87-insertwait-too-eager-skip](bugs/047-x87-insertwait-too-eager-skip/) - WAIT omission heuristic doesn't enumerate all non-waiting FN* ops; sensitive to debug-instruction adjacency | No PR |
| 048 | [048-dynalloca-amount-zero-leaks-mov](bugs/048-dynalloca-amount-zero-leaks-mov/) - `Amount == 0` short-circuit erases the pseudo but not the dead MOV*ri defining its amount vreg | No PR |
| 049 | [049-dynalloca-pushpop-misses-r8-r15-push2](bugs/049-dynalloca-pushpop-misses-r8-r15-push2/) - switch omits APX PUSH2/POP2 (16-byte stack touch), PUSHF*/POPF*, PUSH16*, LEAVE*; can skip probe on Win+APX | No PR |
| 050 | [050-cmov-load-unfold-eflags-clobber-chained](bugs/050-cmov-load-unfold-eflags-clobber-chained/) - chained CMOVrm: `FalseInsertionPoint` set to `FalseMBB->begin()` and never advanced; unfolded loads inserted in reverse → use-before-def | No PR |
| 051 | [051-domain-reassignment-shift-kshift-semantics](bugs/051-domain-reassignment-shift-kshift-semantics/) - `SHR/SHL <8/16/32/64>ri → KSHIFT*ki` have different over-shift semantics: GPR masks to register width, KSHIFT uses full imm8 | No PR |
| 052 | [052-flagscopy-hoist-clobber-window](bugs/052-flagscopy-hoist-clobber-window/) - hoist loop checks HoistMBB's terminators and predecessors' bodies but never scans HoistMBB's own non-terminator body | No PR |
| 053 | [053-flagscopy-splitblock-stale-phi-entries](bugs/053-flagscopy-splitblock-stale-phi-entries/) - `IsEdgeSplit` PHI handling can append duplicate predecessor entries while leaving stale arcs from MBB to same successor | No PR |
| 054 | [054-foldImmediate-copy-class-asymmetry](bugs/054-foldImmediate-copy-class-asymmetry/) - s32 range check gated on SOURCE register class but `NewOpc` chosen from DESTINATION class; cross-class COPY mishandled | No PR |
| 055 | [055-optimizeCmp-narrow-immDelta-signext](bugs/055-optimizeCmp-narrow-immDelta-signext/) - `APInt::getSignedMinValue(BW) == CmpValue` compares APInt via uint64 vs sign-extended int64; misses narrow-width edges | No PR |
| 056 | [056-reMaterialize-MOV32-subreg-write](bugs/056-reMaterialize-MOV32-subreg-write/) - rewrites MOV32r{0,1,_1} to MOV32ri then `substituteRegister(.., SubIdx, ..)` can produce 32-bit write tagged as sub_8/16bit | No PR |
| 057 | [057-fixupvectorconstants-undef-disagreement](bugs/057-fixupvectorconstants-undef-disagreement/) - `extractConstantBits` collapses top-level UndefValue to zero, while `getSplatableConstant` is undef-tolerant; non-idempotent | No PR |
| 058 | [058-mask-cmp-ss-imm-immediate-not-validated](bugs/058-mask-cmp-ss-imm-immediate-not-validated/) - `mask_cmp_ss/sd` shares switch case with legacy `comi/ucomi`; future folds could leak predicate/SAE semantics | No PR |
| 059 | [059-avx512-cur-direction-mxcsr](bugs/059-avx512-cur-direction-mxcsr/) - `x86_avx512_{add,sub,mul,div}_*_512` with R==CUR_DIRECTION folded to plain fadd, discarding MXCSR rounding | No PR |
| 060 | [060-pmulhuw-multiply-by-one-undef-elements](bugs/060-pmulhuw-multiply-by-one-undef-elements/) - `m_One()` matches `<i16 1, i16 undef, ...>`; dropping data dependency on Arg1 for PMULHUW with undef lanes | No PR |
| 062 | [062-fsub-negzero-fneg-snan-sign-flip](bugs/062-fsub-negzero-fneg-snan-sign-flip/) - `fsub -0.0, X → fneg X` without nnan; in-source FIXME confirms sNaN-quieting dropped | No PR |
| 063 | [063-slh-shrx-eflags-bmi2-vector-skip](bugs/063-slh-shrx-eflags-bmi2-vector-skip/) - `saveEFLAGS` skipped whenever BMI2 is present, but mixed vector(gather) + GR64 base may emit flag-clobbering OR64rr while EFLAGS is live | No PR |
| 064 | [064-jumpthreading-distinct-freeze-implied-cond](bugs/064-jumpthreading-distinct-freeze-implied-cond/) - comment claims "exactly the same freeze instruction" but check only compares operands; two distinct `freeze`s independently choose values | No PR |
| 065 | [065-rematerialize-partial-physreg-implicit-def-liveness](bugs/065-rematerialize-partial-physreg-implicit-def-liveness/) - partial physreg rematerialization misses implicit-def liveness, can wrongly drop a still-live def | No PR |
| 066 | [066-stackprotector-tailcall-intervening-instr](bugs/066-stackprotector-tailcall-intervening-instr/) - canary verification insertion can be placed past a tail-call with an intervening instruction | No PR |
| 067 | [067-instcombine-processUGT-bitwidth-eq-bails](bugs/067-instcombine-processUGT-bitwidth-eq-bails/) - bails when CI1 bitwidth equals NewWidth; missed fold / fragile guard | No PR |
| 068 | [068-loopvectorize-anyof-tail-fold-no-mask-on-cmp](bugs/068-loopvectorize-anyof-tail-fold-no-mask-on-cmp/) - `Or(PhiR, Cmp)` lacks AND with HeaderMask under tail folding; poison on inactive lanes can flip reduction true via freeze | No PR |
| 069 | [069-branchfold-optblock-loses-eh-scope-entry](bugs/069-branchfold-optblock-loses-eh-scope-entry/) - MBB→PrevBB splice guards only on `isEHPad`, not `isEHScopeEntry`/`isEHFuncletEntry` | No PR |
| 070 | [070-branchfold-ehscope-empty-skips-itanium-check](bugs/070-branchfold-ehscope-empty-skips-itanium-check/) - cross-EH-scope guard skipped when `EHScopeMembership.empty()` — always true for Itanium DWARF EH (Linux x86_64) | No PR |
| 071 | [071-opt-codegenprepare-pass-segfaults-on-empty-function](bugs/071-opt-codegenprepare-pass-segfaults-on-empty-function/) - `opt -passes=codegenprepare` segfaults on any IR — null-deref in `ProfileSummaryInfo::isFunctionHotInCallGraph` because PSI/BFI not materialized | PR [#199268](https://github.com/llvm/llvm-project/pull/199268) merged |
| 072 | [072-gvn-mssa-loadstore-vn-ignores-atomic-volatile](bugs/072-gvn-mssa-loadstore-vn-ignores-atomic-volatile/) - omits `isVolatile/isAtomic/getOrdering/getSyncScopeID` from the expression key; equal VN, unequal semantics | No PR |
| 073 | [073-newgvn-simplifyselectinst-drops-fmf](bugs/073-newgvn-simplifyselectinst-drops-fmf/) - empty `FastMathFlags()` passed to `simplifySelectInst`; missed FP simplifications | No PR |
| 074 | [074-asmprinter-got-equiv-skips-tls-check](bugs/074-asmprinter-got-equiv-skips-tls-check/) - TLS-relocated globals not excluded from GOT-equivalent collapse | No PR |
| 075 | [075-gisel-matchUndefStore-drops-volatile-atomic](bugs/075-gisel-matchUndefStore-drops-volatile-atomic/) - undef-store elimination doesn't check volatile/atomic — drops user's required side effect | No PR |
| 076 | [076-asmprinter-emitCFI-skip-rbegin-not-isendsection](bugs/076-asmprinter-emitCFI-skip-rbegin-not-isendsection/) - uses MF.rbegin() instead of `isEndSection`; misclassifies skip condition for split functions | No PR |
| 077 | [077-asmprinter-coff-fltused-early-return-skips-morestack](bugs/077-asmprinter-coff-fltused-early-return-skips-morestack/) - early-return on fltused emission skips morestack frame emission | No PR |
| 078 | [078-w38-extloadi64i32-ignores-promote-anyext](bugs/078-w38-extloadi64i32-ignores-promote-anyext/) - ignores `EnablePromoteAnyextLoad` predicate; pattern fires when it should be gated | No PR |
| 079 | [079-w38-gisel-loadi16-loadi32-ignores-promote-anyext](bugs/079-w38-gisel-loadi16-loadi32-ignores-promote-anyext/) - same `EnablePromoteAnyextLoad` predicate omission on GISel side | No PR |
| 080 | [080-w45-asmprinter-modifier-a-A-intel-dialect](bugs/080-w45-asmprinter-modifier-a-A-intel-dialect/) - `'a'`/`'A'` modifiers hardcode AT&T syntax, produce invalid output in Intel-dialect inline asm | No PR |
| 081 | [081-w45-asmprinter-P-modifier-att-intel-asymmetry](bugs/081-w45-asmprinter-P-modifier-att-intel-asymmetry/) - "disp-only" semantics honored by Intel printer but ignored by AT&T printer | No PR |
| 082 | [082-w45-asmparser-lvi-cfi-shl64-in-32-bit-mode](bugs/082-w45-asmparser-lvi-cfi-shl64-in-32-bit-mode/) - always uses `SHL64mi` even matching RET16/32 in 16/32-bit modes — emits REX.W in modes where REX is undefined | No PR |
| 083 | [083-w49-lvi-analyzedefusechain-dead-check-wrong-def](bugs/083-w49-lvi-analyzedefusechain-dead-check-wrong-def/) - dead-check on wrong def — misses an instruction that needs hardening | No PR |
| 084 | [084-w49-lvi-cfg-traverse-skips-instrs-on-revisit](bugs/084-w49-lvi-cfg-traverse-skips-instrs-on-revisit/) - revisits skip instructions; misses hardening sites | No PR |
| 085 | [085-w49-lvi-insertfences-branch-mutates-during-iteration](bugs/085-w49-lvi-insertfences-branch-mutates-during-iteration/) - mutates branch list during iteration — iterator invalidation | No PR |
| 086 | [086-w49-optimize-leas-choose-best-after-MI](bugs/086-w49-optimize-leas-choose-best-after-MI/) - picks the best LEA after MI but doesn't validate operand-classes are equal | No PR |
| 087 | [087-strict-fp-routed-to-fast-libcall](bugs/087-strict-fp-routed-to-fast-libcall/) - strict-fp arithmetic with fast flags routed to FAST_* libcall (e.g. `__hexagon_fast_*`); violates `fpexcept.strict` contract | No PR |
| 088 | [088-scev-howmanylessthans-unsigned-uses-signed-stride-check](bugs/088-scev-howmanylessthans-unsigned-uses-signed-stride-check/) - unsigned variant uses signed stride check; mismatched signedness can yield wrong trip-count | No PR |
| 089 | [089-machine-copy-prop-erase-if-redundant-drops-implicit-operands](bugs/089-machine-copy-prop-erase-if-redundant-drops-implicit-operands/) - erases a COPY that's redundant for explicit defs but carries implicit-defs the user depends on | No PR |
| 090 | [090-vplan-cse-intersect-flags-fmf-wrong-direction](bugs/090-vplan-cse-intersect-flags-fmf-wrong-direction/) - intersects FMF in wrong direction for vector recipes; can promote a less-flexible variant to apply more flags than allowed | No PR |
| 091 | [091-simplifycfg-hoistcondloads-drops-pointer-metadata](bugs/091-simplifycfg-hoistcondloads-drops-pointer-metadata/) - hoisted load drops `!nonnull`/`!dereferenceable` metadata; downstream passes lose info | No PR |
| 092 | [092-fixupsetcc-zu-assert-on-survived-setccr](bugs/092-fixupsetcc-zu-assert-on-survived-setccr/) - accepts both SETCCr and SETZUCCr at filter, then asserts SETZUCCr when ZU; GISel + ZU can trigger | No PR |
| 093 | [093-avx512-vmovs-x86selects-load-fold-mask-suppress](bugs/093-avx512-vmovs-x86selects-load-fold-mask-suppress/) - load-fold pattern under masking misses suppress condition; load may execute on masked lanes | No PR |
| 094 | [094-vex3-to-vex2-xmm16-31-not-rejected](bugs/094-vex3-to-vex2-xmm16-31-not-rejected/) - VEX3→VEX2 shortening doesn't reject XMM16-31; resulting VEX2 encodes wrong register | No PR |
| 095 | [095-mcp-hasimplicitoverlap-misses-implicit-def-of-source](bugs/095-mcp-hasimplicitoverlap-misses-implicit-def-of-source/) - doesn't detect when the source physreg is implicit-def'd between COPY pair | No PR |
| 096 | [096-arg-stack-slot-iterator-invalidated-by-eliminateFI](bugs/096-arg-stack-slot-iterator-invalidated-by-eliminateFI/) - iterator invalidated when eliminateFrameIndex erases/inserts MIs during the rebase walk | No PR |
| 097 | [097-instcombine-vpermilvar-pd-mask-truncates-bit-1](bugs/097-instcombine-vpermilvar-pd-mask-truncates-bit-1/) - mask interpretation uses only bit 0 instead of bit 1 (per Intel spec, PD uses bit 1) | No PR |
| 098 | [098-utils-canCreateUndefOrPoison-missing-div](bugs/098-utils-canCreateUndefOrPoison-missing-div/) - doesn't consider `G_SDIV`/`G_UDIV` etc. as poison-creating; downstream speculative folds unsound | No PR |
| 099 | [099-implicitnullchecks-insertFaultingInstr-loses-mi-flags](bugs/099-implicitnullchecks-insertFaultingInstr-loses-mi-flags/) - doesn't preserve MI flags on the faulting variant; FrameSetup/FrameDestroy/Tail-Call lost | No PR |
| 100 | [100-instcombine-imm-shift-upper-demand-wrong-for-i32](bugs/100-instcombine-imm-shift-upper-demand-wrong-for-i32/) - DemandedUpper mask for i32 shift intrinsics uses wrong bit count | No PR |
| 101 | [101-x86-cleanup-tls-iterator-invalidated](bugs/101-x86-cleanup-tls-iterator-invalidated/) - iterator invalidated by an in-loop erase; possible misclean of TLS_addr sequences | No PR |
| 102 | [102-lower-tile-copy-rax-spill-no-mmo](bugs/102-lower-tile-copy-rax-spill-no-mmo/) - RAX spill/reload `MOV64mr`/`MOV64rm` lack `MachineMemOperand`; post-RA scheduler can reorder around them | No PR |
| 103 | [103-constantfoldfp-host-libm-variance](bugs/103-constantfoldfp-host-libm-variance/) - FP intrinsics (`pow`/`sin`/`atan2`/etc.) constant-folded through build-host libm; last-ULP variance and boundary disagreement | No PR |
| 104 | [104-calllowering-sret-demote-inherits-return-attrs](bugs/104-calllowering-sret-demote-inherits-return-attrs/) - demote-sret pointer ArgInfo inherits return value's SExt/ZExt/InReg/Returned flags | No PR |
| 105 | [105-mirbuilder-buildmasklowptrbits-truncates-wide-ptr](bugs/105-mirbuilder-buildmasklowptrbits-truncates-wide-ptr/) - mask built with `maskTrailingZeros<uint64_t>(NumBits)`; >64-bit pointers get upper mask bits zeroed | No PR |
| 106 | [106-mirbuilder-buildvector-vacuous-assert](bugs/106-mirbuilder-buildvector-vacuous-assert/) - assert `(!SrcOps.empty() \|\| SrcOps.size() < 2)` is always true; intended `>= 2`; same dead guard in G_BUILD_VECTOR_TRUNC, G_CONCAT_VECTORS | No PR |
| 107 | [107-utils-lookthrough-anyext-treats-as-sext](bugs/107-utils-lookthrough-anyext-treats-as-sext/) - with `LookThroughAnyExt=true`, G_ANYEXT reconstructed as `Val.sext()`; constant-folder/codegen disagree on extension | No PR |
| 108 | [108-dse-partial-merge-drops-volatile-atomic](bugs/108-dse-partial-merge-drops-volatile-atomic/) - volatile/atomic killing store dropped + value merged into non-volatile/non-atomic earlier store | PR [#199728](https://github.com/llvm/llvm-project/pull/199728) merged |
| 109 | [109-memcpyopt-memsetmemcpy-drops-volatile-memset](bugs/109-memcpyopt-memsetmemcpy-drops-volatile-memset/) - volatile memset followed by memcpy: original deleted, replacement non-volatile (and equal-size case deletes outright) | No PR |
| 110 | [110-gisel-usube-inverted-borrow-sub128](bugs/110-gisel-usube-inverted-borrow-sub128/) - i128 sub: `setb; cmpb $1` inverts borrow → sbb adds 1 to high half | PR [#199261](https://github.com/llvm/llvm-project/pull/199261) merged |
| 111 | [111-lower-atomic-drops-volatile-rmw-cmpxchg](bugs/111-lower-atomic-drops-volatile-rmw-cmpxchg/) - `lower-atomic` lowers `atomicrmw volatile`/`cmpxchg volatile` into non-volatile load+store; volatile/ordering/syncscope dropped | No PR |
| 113 | [113-avx512-mask-arith-ss-sd-round-cur-direction-mxcsr](bugs/113-avx512-mask-arith-ss-sd-round-cur-direction-mxcsr/) - mask scalar `add/sub/mul/div.ss/sd.round` with rounding=4 (CUR_DIRECTION) folded to plain fadd, losing MXCSR-set rounding | No PR |
| 114 | [114-gvnsink-merges-volatile-stores](bugs/114-gvnsink-merges-volatile-stores/) - volatile is included in expression hash; two equivalent volatile stores merged across branches into one sunk store | No PR |
| 118 | [118-sroa-drops-atomic-ordering](bugs/118-sroa-drops-atomic-ordering/) - predicate `if (LI.isVolatile()) NewLI->setAtomic(...)` should be `isAtomic()`; atomic seq_cst load/store reduced to plain access | No PR |
| 119 | [119-simplifycfg-merge-cond-stores-drops-atomic](bugs/119-simplifycfg-merge-cond-stores-drops-atomic/) - filters via `isUnordered()` (which accepts Unordered atomic) then emits plain `CreateStore`; atomic Unordered → plain store, racy access becomes UB | No PR |
| 120 | [120-simplifycfg-sink-merges-volatile-stores](bugs/120-simplifycfg-sink-merges-volatile-stores/) - two volatile stores in mutually-exclusive branches sunk into one select+volatile-store; static count 2→1 violates LangRef volatile invariant | No PR |
| 121 | [121-simplifycfg-hoist-merges-volatile-loads](bugs/121-simplifycfg-hoist-merges-volatile-loads/) - two volatile loads in mutually-exclusive branches hoisted to one unconditional volatile load | No PR |
| 122 | [122-simplifycfg-hoist-merges-seqcst-atomic-loads](bugs/122-simplifycfg-hoist-merges-seqcst-atomic-loads/) - two `seq_cst` atomic loads in branches hoisted to one unconditional load; conditional→unconditional changes C++ S total order | No PR |
| 124 | [124-atomic-expand-load-to-cmpxchg-drops-volatile-syncscope](bugs/124-atomic-expand-load-to-cmpxchg-drops-volatile-syncscope/) - i128 atomic-volatile load with `singlethread` syncscope → cmpxchg without volatile + system-scope | No PR |
| 125 | [125-atomic-expand-store-to-xchg-drops-volatile-syncscope](bugs/125-atomic-expand-store-to-xchg-drops-volatile-syncscope/) - i128 atomic-volatile store with `singlethread` → cmpxchg loop without volatile + system-scope; also inserts a bare non-volatile load of the dst | No PR |
| 126 | [126-licm-promote-drops-syncscope](bugs/126-licm-promote-drops-syncscope/) - preheader load and exit-block store dropped from `syncscope("singlethread")` to default System scope | No PR |
| 127 | [127-newgvn-call-cse-ignores-operand-bundles](bugs/127-newgvn-call-cse-ignores-operand-bundles/) - call CSE ignores operand bundles (deopt/funclet/ptrauth/kcfi/clang.arc/gc-*); second call with bundle deleted | No PR |
| 128 | [128-lower-matrix-fuseFlatten-drops-volatile](bugs/128-lower-matrix-fuseFlatten-drops-volatile/) - `matrix.column.major.load(..., i1 true /*volatile*/)` rewritten as plain `load <N x float>` — volatile bit dropped | No PR |
| 129 | [129-earlycse-load-cse-ignores-syncscope](bugs/129-earlycse-load-cse-ignores-syncscope/) - atomic unordered loads CSE'd ignoring `SyncScope::ID`; second load takes the cached load's narrower syncscope | No PR |
| 130 | [130-earlycse-dse-stores-ignores-syncscope](bugs/130-earlycse-dse-stores-ignores-syncscope/) - DSE drops earlier atomic store with different syncscope from later one | No PR |
| 132 | [132-atomic-expand-convertcmpxchgtoint-drops-metadata](bugs/132-atomic-expand-convertcmpxchgtoint-drops-metadata/) - cmpxchg ptr → i64 conversion drops `!noalias`/`!tbaa`/`!alias.scope`/`!access_group` metadata | No PR |
| 133 | [133-atomic-expand-rmwcmpxchgloop-initload-drops-metadata](bugs/133-atomic-expand-rmwcmpxchgloop-initload-drops-metadata/) - InitLoaded load doesn't carry the source RMW's metadata; AA-inconsistent view | No PR |
| 134 | [134-atomic-expand-rmw-libcall-drops-volatile-ssid](bugs/134-atomic-expand-rmw-libcall-drops-volatile-ssid/) - libcall (e.g. `__atomic_fetch_nand_16`) silently drops volatile and syncscope | No PR |
| 135 | [135-licm-hoists-fence-out-of-loop](bugs/135-licm-hoists-fence-out-of-loop/) - hoists `fence acquire`/`fence seq_cst` out of loop body; collapses N fences to 1, changing C++ memory model ordering | No PR |
| 137 | [137-sroa-tree-merge-drops-atomic-load](bugs/137-sroa-tree-merge-drops-atomic-load/) - filters `isVolatile()` only; atomic unordered load synthesized via `CreateAlignedLoad` without `setAtomic` | No PR |
| 138 | [138-sroa-vector-promotion-drops-atomic](bugs/138-sroa-vector-promotion-drops-atomic/) - atomic struct store folded into plain zext/shl/or integer-widening output | No PR |
| 139 | [139-cgp-splitBranchCondition-stale-prof-weights](bugs/139-cgp-splitBranchCondition-stale-prof-weights/) - all four `createBranchWeights` calls pass original `(TrueWeight, FalseWeight)` instead of freshly scaled weights | PR [#199822](https://github.com/llvm/llvm-project/pull/199822) not merged |
| 140 | [140-cgp-splitMergedValStore-drops-aa-tbaa-nontemporal](bugs/140-cgp-splitMergedValStore-drops-aa-tbaa-nontemporal/) - dropped `!nontemporal` (no MOVNTI emitted!), `!tbaa`, `!alias.scope`, `!noalias`, `!DIAssignID`, `!annotation` | No PR |
| 141 | [141-branchfold-merges-volatile-and-plain-store](bugs/141-branchfold-merges-volatile-and-plain-store/) - `MachineInstr::isIdenticalTo` ignores MMO flags; volatile store and plain store merged | No PR |
| 142 | [142-machinelicm-isInvariantStore-skips-volatile-atomic](bugs/142-machinelicm-isInvariantStore-skips-volatile-atomic/) - doesn't check `isVolatile()`/`hasOrderedMemoryRef()`; `HoistConstStores=true` by default → volatile/atomic stack-cookie writes hoisted | No PR |
| 143 | [143-memcpyopt-processmemmove-volatile-memset-dropped](bugs/143-memcpyopt-processmemmove-volatile-memset-dropped/) - volatile memset followed by memmove handling | No PR |
| 144 | [144-licm-promote-merges-mismatched-syncscopes](bugs/144-licm-promote-merges-mismatched-syncscopes/) - atomic accesses w/ mismatched syncscopes promoted to single System-scope load+store; original singlethread contract lost | No PR |
| 145 | [145-gvn-pre-drops-loadinst-metadata](bugs/145-gvn-pre-drops-loadinst-metadata/) - hand-rolled metadata whitelist drops `!noundef`/`!align`/`!dereferenceable`/`!nonnull`/`!nontemporal`/`!alias.scope` on PRE'd load | No PR |
| 146 | [146-gvn-createExpr-ignores-IRFlags](bugs/146-gvn-createExpr-ignores-IRFlags/) - CSE keys on (Ty, Opcode, VarArgs) only; kept instr loses `nsw`/`nuw`/`disjoint`/`exact`/`inbounds`/FMF via `patchAndReplaceAllUsesWith` | No PR |
| 147 | [147-jt-duplicateCondBranch-noalias-scopes-not-cloned](bugs/147-jt-duplicateCondBranch-noalias-scopes-not-cloned/) - inline clone loop omits `cloneNoAliasScopes`/`adaptNoAliasScopes` (present in cloneInstructions); duplicated load/store share original scope IDs | No PR |
| 148 | [148-vectorcombine-scalarizeLoadExtract-strips-atomic](bugs/148-vectorcombine-scalarizeLoadExtract-strips-atomic/) - atomic unordered vector load → N plain non-atomic scalar loads; no-torn-read guarantee broken (`-O3` default pipeline) | No PR |
| 149 | [149-dse-partial-merge-drops-nontemporal](bugs/149-dse-partial-merge-drops-nontemporal/) - drops `!nontemporal` metadata when merging (sibling of #108 atomic gap) | No PR |
| 150 | [150-memcpyopt-trymerge-drops-nontemporal-hint](bugs/150-memcpyopt-trymerge-drops-nontemporal-hint/) - bails when start store has `!nontemporal` but inner forward-scan ignores it; subsequent nontemporal stores merged into plain memset, hardware hint lost | No PR |
| 151 | [151-strict-ldexp-i64-libcall-silent-truncation](bugs/151-strict-ldexp-i64-libcall-silent-truncation/) - sibling of #011 for `llvm.experimental.constrained.ldexp.f64.i64` — same silent truncation, lacks FPOWI-style guard | No PR |
| 152 | [152-simplifycfg-sink-merges-two-volatile-seqcst-cmpxchg](bugs/152-simplifycfg-sink-merges-two-volatile-seqcst-cmpxchg/) - volatile seq_cst `cmpxchg` instructions in mutually-exclusive branches sunk into one | No PR |
| 153 | [153-dse-dominating-condition-drops-nontemporal](bugs/153-dse-dominating-condition-drops-nontemporal/) - dropped `!nontemporal` on merged stores | No PR |
| 154 | [154-simplifycfg-sink-merges-two-volatile-seqcst-atomicrmw](bugs/154-simplifycfg-sink-merges-two-volatile-seqcst-atomicrmw/) - volatile seq_cst `atomicrmw` instructions in mutually-exclusive branches sunk into one (sibling of #152) | No PR |
| 155 | [155-frexp-i64-libcall-stack-slot-overrun](bugs/155-frexp-i64-libcall-stack-slot-overrun/) - `llvm.frexp.f64.i64` allocates 8-byte slot, libcall writes 4 (int), load reads 8 — uninitialized upper 4 bytes (info leak + wrong value) | No PR |
| 157 | [157-dse-redundant-stores-of-existing-values-drops-nontemporal](bugs/157-dse-redundant-stores-of-existing-values-drops-nontemporal/) - `isIdenticalToWhenDefined` ignores metadata; merging two identical stores drops `!nontemporal` (different code path from #149/#153) | No PR |
| 158 | [158-sroa-memcpy-split-overrides-load-tbaa-nontemporal](bugs/158-sroa-memcpy-split-overrides-load-tbaa-nontemporal/) - per-load `!tbaa`/`!nontemporal`/`!invariant.load` dropped + memcpy's broader TBAA substituted onto user loads | No PR |
| 159 | [159-sroa-phi-speculate-picks-aa-from-one-user](bugs/159-sroa-phi-speculate-picks-aa-from-one-user/) - AA tag picked from one user of the speculated PHI, applied to merged load — wrong for other users | No PR |
| 160 | [160-licm-promote-merges-store-only-mismatched-syncscopes](bugs/160-licm-promote-merges-store-only-mismatched-syncscopes/) - two unordered atomic STORES w/ mismatched syncscopes merged to one System-scope store; distinct from #144 (load+store) | No PR |
| 161 | [161-licm-promote-merges-load-only-mismatched-syncscopes](bugs/161-licm-promote-merges-load-only-mismatched-syncscopes/) - two unordered atomic LOADS w/ mismatched syncscopes merged to one System-scope load; distinct from #144 | No PR |
| 162 | [162-gvnsink-merges-deopt-bundle-operands-via-phi](bugs/162-gvnsink-merges-deopt-bundle-operands-via-phi/) - non-const deopt bundle operand can be PHI'd; sunk call has per-path deopt value replaced by runtime select | No PR |
| 163 | [163-instcombine-load-retype-drops-invariant-group](bugs/163-instcombine-load-retype-drops-invariant-group/) - `!invariant.group` dropped on load retype; missing case in switch (compare combineMetadata which does handle it) | No PR |
| 164 | [164-mem2reg-convertmetadatatoassumes-drops-range-align-deref](bugs/164-mem2reg-convertmetadatatoassumes-drops-range-align-deref/) - only converts `!nonnull`/`!noundef`; `!range`, `!align`, `!dereferenceable`, `!invariant.load`, AA metadata silently dropped | No PR |
| 165 | [165-instcombine-load-of-select-drops-noundef-invariant-load-nontemporal](bugs/165-instcombine-load-of-select-drops-noundef-invariant-load-nontemporal/) - only copies `Metadata::PoisonGeneratingIDs` to split loads; drops `!noundef`/`!invariant.load`/`!nontemporal`/`!tbaa`/`!alias.scope`/`!dereferenceable` | No PR |
| 167 | [167-gvnhoist-alias-scope-union-extra-membership](bugs/167-gvnhoist-alias-scope-union-extra-membership/) - hoisted load tagged with UNION of `!alias.scope` from both branches; claims membership originals never had → unsound AA queries | No PR |
| 168 | [168-instcombine-unpack-array-load-drops-invariant-load](bugs/168-instcombine-unpack-array-load-drops-invariant-load/) - per-element loads from unpacked array drop `!invariant.load` (and other metadata) | No PR |
| 169 | [169-newgvn-storeexpression-drops-nontemporal](bugs/169-newgvn-storeexpression-drops-nontemporal/) - doesn't compare `!nontemporal`; NT store deleted in favor of plain store, NT hint lost | No PR |
| 170 | [170-newgvn-loadexpression-drops-nontemporal](bugs/170-newgvn-loadexpression-drops-nontemporal/) - doesn't compare `!nontemporal`; CSE merges loads, `combineMetadataForCSE` intersect drops NT hint | No PR |
| 171 | [171-gvnhoist-range-md-union-expands-set](bugs/171-gvnhoist-range-md-union-expands-set/) - hoisted load `!range` is UNION of source ranges, claiming membership in extra range that neither original load had | No PR |
| 172 | [172-gvnhoist-store-nontemporal-silently-dropped](bugs/172-gvnhoist-store-nontemporal-silently-dropped/) - hoisted store drops `!nontemporal` (sibling of NewGVN store-expr bug) | No PR |
| 173 | [173-x86fold-NDDtoRMW-killsRegister-ignores-subreg](bugs/173-x86fold-NDDtoRMW-killsRegister-ignores-subreg/) - `killsRegister` check doesn't account for sub-register kills; can fold over a still-live sub-reg | No PR |
| 174 | [174-atomic-expand-rmwcmpxchgloop-initload-drops-tbaa-noalias](bugs/174-atomic-expand-rmwcmpxchgloop-initload-drops-tbaa-noalias/) - InitLoaded missing `copyMetadataForAtomic`; cmpxchg has tbaa+noalias but seed load doesn't | No PR |
| 175 | [175-atomic-expand-expandPartwordCmpXchg-newCI-drops-tbaa](bugs/175-atomic-expand-expandPartwordCmpXchg-newCI-drops-tbaa/) - widened cmpxchg new CI missing `copyMetadataForAtomic`; sibling widenPartwordAtomicRMW does it correctly | No PR |
| 176 | [176-atomic-expand-convertAtomicLoadToIntegerType-drops-tbaa](bugs/176-atomic-expand-convertAtomicLoadToIntegerType-drops-tbaa/) - drops `!tbaa`/`!noalias`/`!alias.scope`; sibling convertAtomicXchgToIntegerType does it correctly | No PR |
| 177 | [177-instcombine-store-bitcast-drops-invariant-group](bugs/177-instcombine-store-bitcast-drops-invariant-group/) - omits `MD_invariant_group`; `store (bitcast double X to i64) %p, !invariant.group` becomes plain store | No PR |
| 178 | [178-instcombine-store-bitcast-drops-noalias-addrspace](bugs/178-instcombine-store-bitcast-drops-noalias-addrspace/) - omits `MD_noalias_addrspace`; load/store asymmetric (copyMetadataForLoad has it) | No PR |
| 179 | [179-instcombine-load-of-select-drops-invariant-group-tbaa](bugs/179-instcombine-load-of-select-drops-invariant-group-tbaa/) - broader than #165 — also strips `!invariant.group`, `!invariant.load`, `!tbaa`, `!nontemporal`, `!dereferenceable` | No PR |
| 180 | [180-scalarize-masked-mem-drops-metadata-const-mask](bugs/180-scalarize-masked-mem-drops-metadata-const-mask/) - drops `!range`/`!tbaa`/`!noalias`/`!nontemporal`/`!nonnull`/`!dereferenceable`; all-true path correctly copies metadata | No PR |
| 181 | [181-separate-const-offset-from-gep-false-inbounds](bugs/181-separate-const-offset-from-gep-false-inbounds/) - unconditionally `setIsInBounds(true)`; can mark a temporarily-OOB GEP as inbounds → guaranteed poison | PR [#199304](https://github.com/llvm/llvm-project/pull/199304) not merged |
| 182 | [182-simplifycfg-sink-merges-two-fences](bugs/182-simplifycfg-sink-merges-two-fences/) - two `fence` instructions in mutually-exclusive predecessors collapsed to one — one of two release-acquire pairs destroyed | No PR |
| 183 | [183-simplifycfg-hoist-memintrinsic-drops-nontemporal](bugs/183-simplifycfg-hoist-memintrinsic-drops-nontemporal/) - hoisted `llvm.memcpy` drops `!nontemporal` when only one of two carries it (combineMetadataForCSE writes JMD) | No PR |
| 184 | [184-instcombine-atomic-memcpy-memset-loses-element-atomicity](bugs/184-instcombine-atomic-memcpy-memset-loses-element-atomicity/) - element-atomic memcpy/memset with elt=1, len=4 collapsed to single i32 atomic load+store; per-byte atomicity granularity lost | No PR |
| 189 | [189-gvn-processMaskedLoad-drops-return-attrs](bugs/189-gvn-processMaskedLoad-drops-return-attrs/) - replaces masked.load with select but copies no return-value attributes (`nofpclass`, `!range`, `noundef`, `align`, `dereferenceable`) | No PR |
| 190 | [190-vectorcombine-scalarizeLoadBitcast-strips-atomic](bugs/190-vectorcombine-scalarizeLoadBitcast-strips-atomic/) - sibling of #148: atomic vector load that feeds only bitcast users → plain non-atomic scalar load via CreateLoad + copyMetadata | No PR |
| 191 | [191-vectorcombine-scalarizeLoad-infloop-on-strong-atomic](bugs/191-vectorcombine-scalarizeLoad-infloop-on-strong-atomic/) - `monotonic`/`acquire`/`seq_cst` atomic vector load → opt hangs at 100% CPU (worklist re-pushes the surviving original load forever) | No PR |
| 192 | [192-simplifycfg-mergeCondStores-drops-nontemporal](bugs/192-simplifycfg-mergeCondStores-drops-nontemporal/) - merged store drops `!nontemporal` when only one of the paired stores carried it | No PR |
| 193 | [193-simplifycfg-mergeCondStores-spreads-invariant-group](bugs/193-simplifycfg-mergeCondStores-spreads-invariant-group/) - `!invariant.group` from one store leaks onto the merged store carrying the other branch's value | No PR |
| 195 | [195-instcombine-ldexp-chain-integer-overflow](bugs/195-instcombine-ldexp-chain-integer-overflow/) - `ldexp(ldexp(x, INT_MAX), INT_MAX)` → `fmul x, 0.25` (i32 exponent sum wraps to -2 inverting overflow→underflow); should be `+inf` | PR [#199274](https://github.com/llvm/llvm-project/pull/199274) merged |
| 196 | [196-dagcombiner-trystoremergeofloads-drops-aamd](bugs/196-dagcombiner-trystoremergeofloads-drops-aamd/) - merged wide load+store has no `!tbaa`/`!alias.scope`/`!noalias` (4-arg getLoad/getStore overloads drop AAInfo) | No PR |
| 198 | [198-dagcombiner-reduceloadopstorewidth-store-drops-aamd](bugs/198-dagcombiner-reduceloadopstorewidth-store-drops-aamd/) - asymmetric MMO loss: load-side keeps NT/tbaa, store-side drops both — visible in `OR8mi` MMOs | No PR |
| 199 | [199-dagcombiner-combineconsecutiveloads-drops-flags-aainfo](bugs/199-dagcombiner-combineconsecutiveloads-drops-flags-aainfo/) - fused wide load drops MOInvariant + MONonTemporal + AAInfo; disables hoisting/CSE of immutable loads | No PR |
| 200 | [200-memcpyopt-processStoreOfLoad-drops-load-nontemporal-aamd](bugs/200-memcpyopt-processStoreOfLoad-drops-load-nontemporal-aamd/) - load+store→memcpy fold drops load's `!nontemporal`/`!invariant.load` and AAMD; only `DIAssignID` copied | No PR |
| 201 | [201-memcpyopt-processMemCpyMemCpyDependence-drops-nontemporal-aamd](bugs/201-memcpyopt-processMemCpyMemCpyDependence-drops-nontemporal-aamd/) - chained memcpy fold drops `!nontemporal` and AAMD on the surviving memcpy | No PR |
| 202 | [202-scalarize-masked-gather-dynamic-drops-metadata](bugs/202-scalarize-masked-gather-dynamic-drops-metadata/) - per-lane loads drop ALL metadata (!range/!nontemporal/!noalias/!alias.scope/...) — downstream instcombine `!range` fold fails | No PR |
| 203 | [203-scalarize-masked-scatter-dynamic-drops-nontemporal](bugs/203-scalarize-masked-scatter-dynamic-drops-nontemporal/) - per-lane stores drop `!nontemporal` (and AAMD); backend emits cached MOV instead of MOVNT* | No PR |
| 204 | [204-scalarize-masked-expandload-drops-nontemporal](bugs/204-scalarize-masked-expandload-drops-nontemporal/) - both const- and dyn-mask paths drop `!nontemporal`/AAMD on per-lane loads (no all-true short-cut) | No PR |
| 205 | [205-scalarize-masked-compressstore-drops-nontemporal](bugs/205-scalarize-masked-compressstore-drops-nontemporal/) - mirror of #204 — per-lane stores drop NT/AAMD on both const- and dyn-mask paths | No PR |
| 206 | [206-simplifylibcalls-fmod-incorrect-nnan-on-frem](bugs/206-simplifylibcalls-fmod-incorrect-nnan-on-frem/) - `fmod(NaN, 1.0)` folded to `frem nnan NaN, 1.0` → **poison**; `IsNoNan` proof actually checks no-errno not no-NaN | PR [#199284](https://github.com/llvm/llvm-project/pull/199284) merged |
| 207 | [207-simplifylibcalls-fdim-inf-minus-inf-qnan](bugs/207-simplifylibcalls-fdim-inf-minus-inf-qnan/) - `fdim(±Inf, ±Inf)` folds to qNaN instead of `+0.0` per C99 (uses `max(X-Y, 0)` instead of comparison-first definition) | PR [#199306](https://github.com/llvm/llvm-project/pull/199306) merged |
| 208 | [208-sdag-memcpy-lowering-drops-nontemporal](bugs/208-sdag-memcpy-lowering-drops-nontemporal/) - `llvm.memcpy ..., !nontemporal` → per-chunk MMOs with no MONonTemporal; x86 emits cached MOV* instead of MOVNT* | No PR |
| 209 | [209-sdag-memmove-lowering-drops-nontemporal](bugs/209-sdag-memmove-lowering-drops-nontemporal/) - sister of #208 for memmove | No PR |
| 210 | [210-sdag-memset-lowering-drops-nontemporal](bugs/210-sdag-memset-lowering-drops-nontemporal/) - sister of #208 for memset | No PR |
| 211 | [211-instcombine-unpack-struct-load-drops-nontemporal](bugs/211-instcombine-unpack-struct-load-drops-nontemporal/) - per-field `load i32` instructions don't inherit `!nontemporal`/`!access_group` from aggregate load | No PR |
| 212 | [212-instcombine-unpack-struct-store-drops-nontemporal](bugs/212-instcombine-unpack-struct-store-drops-nontemporal/) - mirror of #211 for stores | No PR |
| 213 | [213-legalize-expandintres-load-drops-range](bugs/213-legalize-expandintres-load-drops-range/) - i128 load split into two i64 loads drops `!range` on both MMOs | No PR |
| 214 | [214-jumpthreading-unfoldselect-drops-unpredictable](bugs/214-jumpthreading-unfoldselect-drops-unpredictable/) - `select !unpredictable` → `br` drops `!unpredictable` (pass never references `MD_unpredictable`) | No PR |
| 217 | [217-lowerinvoke-drops-invoke-metadata](bugs/217-lowerinvoke-drops-invoke-metadata/) - new `CallInst` lacks `copyMetadata`; drops `!prof`/`!annotation`/`!range`/`!callees`/`!nosanitize`/`!noalias`/`!alias.scope` | No PR |
| 218 | [218-verifier-vp-profile-null-deref-crash](bugs/218-verifier-vp-profile-null-deref-crash/) - malformed `!prof !{"VP", i32 0, i64 100, !"oops", i64 50}` triggers null-deref crash in verifier (`getZExtValue()` on null dyn_extract) | PR [#199170](https://github.com/llvm/llvm-project/pull/199170) merged |
| 219 | [219-combinemetadata-drops-j-only-tbaa](bugs/219-combinemetadata-drops-j-only-tbaa/) - iterates only K's metadata via `getAllMetadataOtherThanDebugLoc`; any kind J-only (e.g., `!tbaa`) silently dropped during EarlyCSE/GVN/SimplifyCFG | No PR |
| 221 | [221-instcombine-mergeStoreIntoSuccessor-drops-nontemporal](bugs/221-instcombine-mergeStoreIntoSuccessor-drops-nontemporal/) - two `!nontemporal` stores in successor blocks merged into single store; new store gets no metadata (only dbg/DIAssignID/AAMetadata transferred) | No PR |
| 222 | [222-expand-ir-insts-scalarize-ice-on-fpto_sat-vector](bugs/222-expand-ir-insts-scalarize-ice-on-fpto_sat-vector/) - ICE on `<2 x i256> @llvm.fptoui.sat.v2i256.v2f32`; dispatcher enqueues IntrinsicInst but scalarize only handles BinaryOperator/CastInst | PR [#199174](https://github.com/llvm/llvm-project/pull/199174) merged |
| 223 | [223-expand-ir-insts-fpto-sat-inf-not-saturated](bugs/223-expand-ir-insts-fpto-sat-inf-not-saturated/) - `fptoui.sat.i256.f32(+Inf)` produces ~2^128 instead of UINT256_MAX; threshold `BitWidth-IsSigned` ≥ FP exponent max never holds for wide ints | No PR |
| 224 | [224-sdagbuilder-atomicrmw-cmpxchg-drops-align-aamd](bugs/224-sdagbuilder-atomicrmw-cmpxchg-drops-align-aamd/) - uses `getEVTAlign(MemVT)` instead of `I.getAlign()`; `atomicrmw add align 32` produces MMO `alignment: 1` | No PR |
| 225 | [225-loopunroll-loadcse-drops-nontemporal](bugs/225-loopunroll-loadcse-drops-nontemporal/) - RAUW merging same-address loads in unrolled iterations drops `!nontemporal`/`!align` (no combineMetadataForCSE) | No PR |
| 226 | [226-branchfolding-tail-merge-drops-atomic-ordering](bugs/226-branchfolding-tail-merge-drops-atomic-ordering/) - tail-merges `load atomic monotonic` with plain `load`; result is plain (monotonic dropped) | PR [#199892](https://github.com/llvm/llvm-project/pull/199892) merged |
| 227 | [227-atomicexpand-vector-load-to-cmpxchg-verifier-crash](bugs/227-atomicexpand-vector-load-to-cmpxchg-verifier-crash/) - `load atomic <2 x i64>` (+cx16) synthesizes illegal `cmpxchg ptr, vec, vec`; verifier rejects → hard crash | PR [#199310](https://github.com/llvm/llvm-project/pull/199310) merged |
| 228 | [228-gvn-pre-drops-load-metadata-whitelist](bugs/228-gvn-pre-drops-load-metadata-whitelist/) - hand-rolled metadata whitelist drops `!nonnull`/`!dereferenceable`/`!align`/`!noundef`/`!nontemporal`/`!fpmath` on PRE'd load (verified at default -O2) | No PR |
| 229 | [229-gvn-earlycse-cse-strips-nontemporal-from-stationary-leader](bugs/229-gvn-earlycse-cse-strips-nontemporal-from-stationary-leader/) - unconditional `setMetadata(JMD)` strips NT from stationary CSE leader when sibling lacks it; fires in GVN AND EarlyCSE | No PR |
| 230 | [230-gvn-earlycse-cse-strips-nosanitize](bugs/230-gvn-earlycse-cse-strips-nosanitize/) - same shape as #229 for `!nosanitize`; CSE'd leader loses no-instrumentation hint, sanitizers may re-instrument | No PR |
| 231 | [231-branchfolding-tail-merge-strengthens-miflags](bugs/231-branchfolding-tail-merge-strengthens-miflags/) - `nuw add` + plain `add` in tail-merge candidates → single `nuw add` on both paths; strengthens flag (unsound direction) | No PR |
| 232 | [232-simple-loop-unswitch-zeroes-default-weight](bugs/232-simple-loop-unswitch-zeroes-default-weight/) - switch with `!prof {branch_weights, "expected", 100, 1, 1}` unswitched to `{branch_weights, 0, 1, 1}` — default weight zeroed | No PR |
| 234 | [234-instsimplify-strictfp-poison-fold-drops-side-effect](bugs/234-instsimplify-strictfp-poison-fold-drops-side-effect/) - strict-FP `constrained.fadd nnan` with sNaN folded to NaN literal; FE_INVALID exception side-effect elided | PR [#199405](https://github.com/llvm/llvm-project/pull/199405) not merged |
| 235 | [235-slpvectorizer-select-drops-fmf-prof](bugs/235-slpvectorizer-select-drops-fmf-prof/) - 4 scalar `select nnan` lanes merged into `<4 x i1>` select with no `nnan`/`!prof`/`!unpredictable` (no propagateIRFlags call) | No PR |
| 237 | [237-machinecse-drops-mmo-on-erase](bugs/237-machinecse-drops-mmo-on-erase/) - CSE'd MachineInstr load loses sibling's `!range`/AAInfo on erase (no `cloneMergedMemRefs`) | No PR |
| 238 | [238-branchfolding-tail-merge-narrows-syncscope](bugs/238-branchfolding-tail-merge-narrows-syncscope/) - system-scope atomic store + `syncscope("singlethread")` atomic store tail-merged → system path silently narrowed to singlethread | PR [#199892](https://github.com/llvm/llvm-project/pull/199892) merged |
| 239 | [239-machinelateinstrscleanup-isidenticalto-ignores-mmos](bugs/239-machinelateinstrscleanup-isidenticalto-ignores-mmos/) - merged invariant-load survivor drops `!nontemporal` MMO flag (uses `isIdenticalTo` which ignores MMOs) | No PR |
| 240 | [240-x86-inline-probe-stack-skips-full-page-alloca](bugs/240-x86-inline-probe-stack-skips-full-page-alloca/) - one-page (4096-byte) alloca with `probe-stack="inline-asm"` emits `subq $4096, %rsp` WITHOUT any probe; defeats stack-clash protection | No PR |
| 241 | [241-instcombine-buildNew-shuffle-reorder-drops-cmp-cast-flags](bugs/241-instcombine-buildNew-shuffle-reorder-drops-cmp-cast-flags/) - shuffle reorder of `icmp samesign`/`fcmp nnan ninf`/`zext nneg`/`trunc nuw` drops the flag on new instr | No PR |
| 242 | [242-aic-foldConsecutiveLoads-drops-load-metadata](bugs/242-aic-foldConsecutiveLoads-drops-load-metadata/) - merged wide load loses `!nontemporal`/`!invariant.load`/`!noundef`; only AAMD propagated | No PR |
| 243 | [243-lcssa-exit-phi-drops-fmf](bugs/243-lcssa-exit-phi-drops-fmf/) - `%y.lcssa = phi float [ %y, %h ]` lacks `nnan ninf nsz reassoc` even though `%y` (only incoming) carries them | No PR |
| 244 | [244-scev-expander-drops-inrange-inbounds-on-gep](bugs/244-scev-expander-drops-inrange-inbounds-on-gep/) - synthesized `%scevgep = getelementptr i8, ...` loses `inbounds` AND `inrange(-8, 24)` from source GEP | No PR |
| 245 | [245-instcombine-unpack-aggregate-drops-nontemporal-extra](bugs/245-instcombine-unpack-aggregate-drops-nontemporal-extra/) - full enumeration of dropped kinds: `!nontemporal`, `!access_group`, `!invariant.group`, `!mem_parallel_loop_access`, `!DIAssignID` | No PR |
| 246 | [246-constantfolding-ldexp-i64-exponent-narrowed-to-int](bugs/246-constantfolding-ldexp-i64-exponent-narrowed-to-int/) - `ldexp(1.0, i64 4294967330)` folded to `2^34` (i64→int narrowing wraps); expected `+inf` per LangRef | PR [#199309](https://github.com/llvm/llvm-project/pull/199309) merged |
| 249 | [249-function-attrs-ignores-operand-bundles](bugs/249-function-attrs-ignores-operand-bundles/) - predicates use `CallBase::hasFnAttr` which ignores operand bundles; caller with `[ "side_effects"() ]` on leaf still infers `nofree nosync nounwind willreturn` | No PR |
| 250 | [250-simplifycfg-mergeConditionalStoreToAddress-drops-pstore-metadata](bugs/250-simplifycfg-mergeConditionalStoreToAddress-drops-pstore-metadata/) - asymmetric combineMetadata + `SI->copyMetadata(*QStore)` drops PStore-only `!nontemporal`/`!tbaa`/...; `!invariant.group` special-case can taint merged store | No PR |
| 252 | [252-jumpthreading-unfoldSelectInstr-branches-on-poison](bugs/252-jumpthreading-unfoldSelectInstr-branches-on-poison/) - original safely freezes potentially-poison condition before branching; after JT, the freeze is gone and `br i1 %maybe_poison` is direct UB | PR [#199408](https://github.com/llvm/llvm-project/pull/199408) merged |

## Coverage notes

### Worker w71 (LoopVectorize)
- Hunted LV miscompiles via C-level random fuzz, IR-level random fuzz, and pattern-targeted tests
- Compared O0 vs O2; also O2 vs O2 with -fno-vectorize/-fno-slp-vectorize as reference
- Patterns probed: tail-folded reductions, predicated div/rem, first-order recurrence, stride-3/5 interleave, min/max reduction with index (FindLast), early-exit, anyOf, gather/scatter, conditional store, conditional load, alignment edges, multiple inductions, reverse iteration, u8 wrap accum, FP min/max
- Flag combos: default O2; predicate-dont-vectorize; force-vector-width 2/4/8/16/32; force-vector-interleave 2/4/8; -mavx2; -mavx512f/vl/bw/dq; -enable-masked-interleaved-mem-accesses; -enable-early-exit-vectorization
- Initial integer C-fuzz found 13 mismatches (signed-overflow / INT_MIN/-1 UB — disappeared with -fwrapv, not LV bugs)
- FP fuzz with -ffast-math found 122 mismatches but all persist with -fno-vectorize (generic FP reassoc, not LV)
- After UB-filtering: 0 confirmed LV miscompiles in ~12 minutes
- Conclusion: LoopVectorize at default O2 is robust against these patterns; no bugs added
