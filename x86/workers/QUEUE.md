# Worker queue

Wave-1 assignments (in flight): w01-w12, see history.

## Wave 2 (launch as wave 1 slots free)

- w13: X86ISelDAGToDAG.cpp — full file. Look for: AM matcher off-by-one in displacement range, wrong segment register handling, RIP-relative misses, address-matcher accepting an i32 displacement that needs sign-extension to i64, X86::AND8ri vs AND32ri imm-fits checks, sub-register fold of TRUNCATE into a load.
- w14: GISel/X86InstructionSelector.cpp + GISel/X86LegalizerInfo.cpp + GISel/X86CallLowering.cpp
- w15: X86FastISel.cpp — fast-path miscompiles, especially around fp-to-int / int-to-fp / sign-ext into wide reg.
- w16: X86InstCombineIntrinsic.cpp PART 2 — re-read with focus on AVX-512 masked intrinsics, kmask intrinsics, embedded rounding.
- w17: X86TargetTransformInfo.cpp — cost model misreports lead to bad SLP/Vectorize decisions but rarely a miscompile; focus on `instCombineIntrinsic`, `simplifyDemandedVectorEltsIntrinsic`, `simplifyDemandedUseBitsIntrinsic` overrides for x86 intrinsics.
- w18: X86PartialReduction.cpp + X86InsertPrefetch.cpp + X86InterleavedAccess.cpp + X86OptimizeLEAs.cpp
- w19: X86SpeculativeLoadHardening.cpp + X86SpeculativeExecutionSideEffectSuppression.cpp + X86LoadValueInjectionLoadHardening.cpp + X86LoadValueInjectionRetHardening.cpp
- w20: X86IndirectBranchTracking.cpp + X86IndirectThunks.cpp + X86ReturnThunks.cpp + X86KCFI.cpp
- w21: X86WinEHState.cpp + X86WinEHUnwindV2.cpp + X86WinFixupBufferSecurityCheck.cpp + X86PreTileConfig.cpp + X86TileConfig.cpp + X86LowerAMXType.cpp + X86LowerAMXIntrinsics.cpp + X86PreAMXConfig.cpp
- w22: SelectionDAG/SelectionDAGBuilder.cpp x86-relevant intrinsics (focus on lowerCallTo + visitIntrinsicCall + visitAtomicLoad/Store)
- w23: SelectionDAG/LegalizeVectorOps.cpp + LegalizeVectorTypes.cpp + LegalizeIntegerTypes.cpp (focus on type-legalization splits and widens)
- w24: SelectionDAG/SelectionDAG.cpp + TargetLowering.cpp — generic helpers like SimplifyDemandedBits, SimplifyDemandedVectorElts, SimplifySetCC

## Wave 3 (later)

- w25: CodeGen/AtomicExpandPass.cpp + CodeGen/ExpandReductions.cpp + CodeGen/ExpandLargeFpConvert.cpp + CodeGen/ExpandLargeDivRem.cpp
- w26: CodeGen/CodeGenPrepare.cpp
- w27: Transforms/InstCombine/InstCombineCalls.cpp (x86-relevant)
- w28: Transforms/InstCombine/InstCombineSimplifyDemanded.cpp
- w29: Transforms/InstCombine/InstCombineCompares.cpp + InstCombineCasts.cpp
- w30: Transforms/Vectorize/VectorCombine.cpp
- w31: Transforms/Vectorize/SLPVectorizer.cpp
- w32: Transforms/Vectorize/LoopVectorize.cpp focus on tail-folding & masked memops
- w33: Transforms/Scalar/GVN.cpp + EarlyCSE.cpp + NewGVN.cpp
- w34: Transforms/Scalar/JumpThreading.cpp + CorrelatedValuePropagation.cpp + SCCP.cpp + IndVarSimplify.cpp
- w35: CodeGen/MachineSink.cpp + MachineLICM.cpp + MachineCSE.cpp + PeepholeOptimizer.cpp
- w36: CodeGen/RegisterCoalescer.cpp + LiveIntervals.cpp + LiveVariables.cpp
- w37: CodeGen/BranchFolding.cpp + TailDuplicator.cpp + IfConversion.cpp
- w38: CodeGen/StackColoring.cpp + LocalStackSlotAllocation.cpp + StackProtector.cpp + ShrinkWrap.cpp
- w39: CodeGen/PrologEpilogInserter.cpp + RegisterScavenging.cpp
- w40: CodeGen/MachineOutliner.cpp + X86Outliner integration
- w41: CodeGen/MIRSampleProfile.cpp + AssignmentTrackingAnalysis.cpp
- w42: X86 .td: X86InstrInfo.td + X86InstrAVX512.td + X86InstrSSE.td predicates and isel patterns
- w43: X86 .td: X86InstrCompiler.td + X86InstrFMA.td + X86InstrShiftRotate.td
- w44: X86 .td: X86InstrFragmentsSIMD.td + X86InstrVecCompiler.td + X86InstrFragments.td
- w45: MC/X86 encoding (X86MCCodeEmitter, X86AsmBackend, X86InstrRelaxTables)
- w46: X86Schedule*.td + X86InstrInfo scheduling overrides — rarely causes miscompiles but check
- w47: ASAN/MSAN/TSAN instrumentation passes (in pipeline by default with sanitizers, only file if non-sanitizer paths affected)
- w48: SelectionDAG/DAGCombiner remaining + DAGCombiner shuffle helpers we didn't cover
- w49: X86CodeGenPassBuilder + X86TargetMachine pass order — wrong ordering can cause real bugs
- w50: Free slot for any especially fertile area discovered during triage
