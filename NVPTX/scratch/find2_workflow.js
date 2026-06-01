export const meta = {
  name: 'nvptx-find-miscompiles-round2',
  description: 'Class-based sweep of the NVPTX backend for sibling miscompiles/crashes of round-1 finds',
  phases: [{ title: 'ClassSweep' }],
}

const SRC = '/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX'
const LLC = '/Users/justinlebar/code/llvm2/build/bin/llc'

const BAR = `
You are hunting CORRECTNESS BUGS in the LLVM NVPTX backend, sweeping a whole BUG CLASS across the backend.

WHAT COUNTS (priority order): (1) MISCOMPILES — emitted PTX/MIR computes a different result than the input IR for a well-defined (non-UB) input; (2) compiler SEGFAULTS/OOB/UAF reachable from valid input; (3) (low value) assertion failures from valid IR.

WHAT DOESN'T COUNT: "cannot select"/unsupported/report_fatal_error on genuinely unsupported ops; dropped metadata; missed optimizations; perf; style.

These confirmed round-1 bugs define the *classes* you are sweeping for SIBLINGS of:
- Unguarded cast<ConstantInt>/cast<ConstantSDNode> on an intrinsic operand that is NOT marked ImmArg in the .td -> crash on a runtime operand (e.g. nvvm_ldu_global align).
- getTargetConstant(val.getZExtValue(), DL, MVT::i32) where the operand / instruction field is actually i64 -> high bits truncated (e.g. tcgen05.ld offset).
- A signed widening fold that fabricates a constant in the narrow type whose top bit is set, then feeds mul.wide.s (sign-extends), negating the value (combineMulWide sext(shl nsw x, bits-1)).
- A guarded shift folded to a PTX clamp shift using only the low 32 bits of a wider shift amount (PerformSELECTShiftCombine i64).
- float->iN conversion lowered with the wrong operation (fp_to_int to i1 lowered as integer setp.eq 0).
- int->narrow-fp (bf16) lowered as int->f32->bf16, double-rounding.
- An InstVisitor/PtrUseVisitor/SDNode visitor whose unhandled cases fall to a no-op default and get MISclassified (NVPTXLowerArgs ArgUseChecker treating icmp/atomicrmw as read-only).
- byval/grid_constant param handling that marks a written pointer readonly or skips the per-thread local copy.
- Sub-byte / vector constant emission overflowing or mis-packing the AsmPrinter buffer.

RIGOR: For each finding give a SPECIFIC defined input and the exact wrong result (or the exact crash). Trace the code. Empty findings array is a fine answer if your class has no real siblings. You MAY use the built llc at ${LLC} to confirm (optional but encouraged for this round, since you have concrete patterns).

For each finding provide: title; file+lines; kind (miscompile|segfault|assertion|other); mechanism (with code excerpt + why the result is wrong); trigger; ir (minimal self-contained LLVM IR for llc -mtriple=nvptx64); llc_cmd; confidence (0-1).
`

const SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: {
    findings: { type: 'array', items: {
      type: 'object', additionalProperties: false,
      properties: {
        title: { type: 'string' }, file: { type: 'string' }, lines: { type: 'string' },
        kind: { type: 'string', enum: ['miscompile', 'segfault', 'assertion', 'other'] },
        mechanism: { type: 'string' }, trigger: { type: 'string' }, ir: { type: 'string' },
        llc_cmd: { type: 'string' }, confidence: { type: 'number' },
        confirmed_with_llc: { type: 'boolean' },
      },
      required: ['title', 'file', 'lines', 'kind', 'mechanism', 'trigger', 'ir', 'llc_cmd', 'confidence', 'confirmed_with_llc'],
    } },
  },
  required: ['findings'],
}

const CLASSES = [
  { key: 'C1-unguarded-constantint-casts',
    hint: `Find every unguarded cast<ConstantInt>(...) / cast<ConstantSDNode>(...) / cast<ConstantFPSDNode>(...) applied to an intrinsic call operand (I.getArgOperand(n) or N->getOperand(n)) in NVPTXISelLowering.cpp (getTgtMemIntrinsic, lowerIntrinsic*, getTgtMemIntrinsic switch) and NVPTXISelDAGToDAG.cpp (tryIntrinsic*, Select*). For each, check the matching intrinsic in IntrinsicsNVVM.td: is that operand marked ImmArg (so it MUST be a constant) or not? If NOT ImmArg, a runtime operand crashes the cast (asserts build) or yields garbage (release). Report each non-ImmArg operand read via a checked cast as a crash. Grep: 'cast<ConstantInt>', 'cast<ConstantSDNode>', 'getArgOperand', 'getConstantOperandVal'.` },
  { key: 'C2-truncating-immediate-width',
    hint: `Find every getTargetConstant(... , MVT::i32) / getConstant(..., MVT::i16/i8) / CurDAG->getTargetConstant(X.getZExtValue(), ..., MVT::iNN) in NVPTXISelDAGToDAG.cpp and NVPTXISelLowering.cpp where the source value (an intrinsic immarg or SDValue) is WIDER than the chosen MVT, so high bits are dropped. Cross-check the operand's declared type in IntrinsicsNVVM.td and the instruction field width in NVPTXIntrinsics.td/NVPTXInstrInfo.td (i64imm vs i32imm). Sibling of the tcgen05.ld offset truncation. Grep: 'getTargetConstant', 'getZExtValue', 'MVT::i32'.` },
  { key: 'C3-sign-zero-extension',
    hint: `Audit sign/zero-extension correctness in: combineMulWide / TryMULWIDECombine / mul.wide patterns; tryBFE (bit-field-extract start/width/signedness, SExt vs ZExt); extending load/store selection in tryLoad/tryStore (which fromType / sign chosen); SETCC/zext-of-setcc; any place building a constant in a narrow type then sign-extending. Report cases where the emitted signedness differs from IR. Sibling of combineMulWide bug. Files: NVPTXISelLowering.cpp, NVPTXISelDAGToDAG.cpp, NVPTXInstrInfo.td.` },
  { key: 'C4-asmprinter-const-emission',
    hint: `Audit ALL of NVPTXAsmPrinter.cpp constant emission: bufferAggregateConstant, bufferAggregateConstVec, bufferLEByte/AddIntToBuffer, printScalarConstant, emitGlobals, struct padding insertion, ConstantDataSequential handling, ConstantExpr, big vs little endian byte order, sub-byte packing, and the AggBuffer size computation. Look for any path where emitted bytes != the constant's true bytes (wrong layout) or the byte count exceeds the AggBuffer Size (OOB). Siblings of the <8 x i4> global and splat bugs. Try several global shapes with llc.` },
  { key: 'C5-incomplete-visitors',
    hint: `Find InstVisitor / PtrUseVisitor / SDNode-switch / TypeSwitch based code in the NVPTX backend whose UNHANDLED cases fall through to a no-op default or a wrong default, causing misclassification or a missed transform that changes semantics. Especially: NVPTXLowerArgs (ArgUseChecker, CloneInstInParamAS), NVPTXLowerAlloca, NVPTXImageOptimizer, NVPTXForwardParams, any visitor with llvm_unreachable("Unsupported") reachable from a checker that classified the value as safe. Sibling of the ArgUseChecker icmp/atomicrmw crash.` },
  { key: 'C6-shift-rotate-clamp',
    hint: `Audit shift/rotate/funnel-shift lowering & combines for shift-amount handling: LowerShiftLeftParts/RightParts, lowerFSH/expandFSH64/lowerROT, FSHL_CLAMP/FSHR_CLAMP, SHL_CLAMP/SRL_CLAMP, PerformSELECTShiftCombine, and any pattern relying on PTX shift-amount clamp/modulo. Look for: shift amount taken modulo wrong width, clamp assumed but amount wider than 32 bits, rotate amount not reduced, missing handling of amount==0 or amount>=width. Sibling of the i64 guarded-shift clamp bug.` },
  { key: 'C7-fp-conversion-rounding',
    hint: `Audit FP conversion/arith lowering for rounding/FTZ/double-rounding errors: LowerINT_TO_FP, LowerFP_TO_INT, LowerFP_ROUND, LowerFP_EXTEND, FROUND32/64, PromoteBinOpToF32 (compute in f32 then round - double rounding for f16/bf16?), bf16/f16 conversion chains, fdiv/fsqrt/frem approximations used without the right flags. Report cases producing a value other than the correctly-rounded IR result for a concrete input. Sibling of int->bf16 double-round bug. Files: NVPTXISelLowering.cpp, NVPTXInstrInfo.td (cvt patterns).` },
  { key: 'C8-byval-paramspace',
    hint: `Audit NVPTXLowerArgs.cpp + NVPTXSetByValParamAlign.cpp + NVPTXForwardParams.cpp for byval/grid_constant/param-space correctness: marking a pointer readonly when it can be written; cvta.param aliasing the shared param copy without a per-thread local copy where required; wrong address space; alignment. Sibling of the byval-atomicrmw-marked-readonly bug. Enumerate the cases in lowerKernelByValParam (case 1/2/3) and which uses each allows.` },
  { key: 'C9-memcpy-memmove-aggr',
    hint: `Audit NVPTXLowerAggrCopies.cpp + memcpy/memmove/memset lowering (createMemCpyLoopKnownSize/expandMemMoveAsLoop usage, CanOverlap, element size, volatile, atomic, address spaces) for overlap/size/volatile correctness. Sibling of the overlapping aggregate load/store forward-copy bug. Check: does any load/store or memcpy path that may overlap use a forward-only copy; is volatile/atomic preserved; is the byte count correct.` },
  { key: 'C10-atomic-fence-ordering',
    hint: `Audit atomic lowering: NVPTXAtomicLower.cpp, tryFence, selectAtomicSwap128, shouldInsertFencesForAtomic, scoped atomic mapping (.sys/.gpu/.cta), atom op selection in NVPTXIntrinsics.td, 128-bit atomic hi/lo ordering. Look for: wrong memory-ordering/scope qualifier emitted, missing fence, atom mapped to the wrong operation, hi/lo swapped, runtime-value scoped atomic mishandled. Report semantic mismatches with IR.` },
  { key: 'C11-vector-lane-order',
    hint: `Audit vector lane ordering/packing for v2f16/v2bf16/v2i16/v4i8 and 128-bit splits: LowerBUILD_VECTOR, LowerEXTRACT/INSERT_VECTOR_ELT, LowerVECTOR_SHUFFLE, tryUNPACK_VECTOR, tryEXTRACT_VECTOR_ELEMENT, lowerLoadVector/lowerSTOREVector, SelectV2I64toI128/SelectI128toV2I64, combinePackingMovIntoStore, performScalarizeV2F32Op. Look for a lane index swapped (hi/lo), wrong element picked, or endian/order mismatch that changes results. Confirm with llc + extractelement.` },
  { key: 'C12-combine-missing-flags',
    hint: `Audit DAGCombines in NVPTXISelLowering.cpp that may fire WITHOUT the required IR flags or that drop poison/refinement: PerformADDCombine, performFADDCombineWithOperands/FMA fusion (needs contract/fast?), combineMADConstOne/combineMulSelectConstOne (x*(C+1) rewrites), PerformSHLCombine, PerformSETCCCombine, PerformEXTRACTCombine, combinePRMT, combineF16AddWithNeg (a+(-b)->sub operand order), PerformVSELECTCombine, combineADDRSPACECAST. Look for a fold that changes the numeric result for a defined input (wrong identity, wrong operand order, missing nsw/nuw/fast guard, poison introduced).` },
]

phase('ClassSweep')

const results = await parallel(CLASSES.map(c => () =>
  agent(`${BAR}\n\n=== YOUR BUG CLASS ===\n${c.hint}\n\nSweep ${SRC} (read files, grep for the patterns). Examine EVERY hit, not just the first. Return findings via structured output.`,
    { label: c.key, phase: 'ClassSweep', schema: SCHEMA })
    .then(r => ({ key: c.key, findings: (r && r.findings) || [] }))
    .catch(() => ({ key: c.key, findings: [] }))
))

const all = []
for (const r of results) for (const f of (r.findings || [])) all.push({ region: r.key, ...f })
all.sort((a, b) => (b.confidence || 0) - (a.confidence || 0))
log(`Round2: ${all.length} raw findings across ${CLASSES.length} classes`)
return { count: all.length, findings: all }
