export const meta = {
  name: 'nvptx-find-miscompiles-round3',
  description: 'Deep + under-explored sweep of the NVPTX backend for more miscompiles/crashes (target: reach 30 total)',
  phases: [{ title: 'DeepSweep' }],
}

const SRC = '/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX'
const INC = '/Users/justinlebar/code/vm-shared/llvm/llvm/include/llvm/IR'
const LLC = '/Users/justinlebar/code/llvm2/build/bin/llc'

const KNOWN = `
ALREADY-FOUND bugs (do NOT re-report these; find DIFFERENT ones):
1. fptoui/fptosi float->i1 lowered as setp.eq 0 (NVPTXInstrInfo.td SETP i1 patterns).
2. combineMulWide: sext(shl nsw x, bits-1) -> mul.wide.s by negative const (ISelLowering ~6379).
3. PerformSELECTShiftCombine: i64 guarded shift clamp uses only low 32 bits (ISelLowering ~6766).
4. int->bf16 double-rounds via f32 (LowerINT_TO_FP ~2461).
5. byval kernel param used by icmp/freeze/atomicrmw/cmpxchg: ArgUseChecker no-op default misclassifies as readonly -> crash at NVPTXLowerArgs.cpp:256 / cvta.param miscompile (NVPTXLowerArgs.cpp).
6. va_arg of i8/i1 advances va_list by 2B while caller packs 1B (LowerVAARG ~3574).
7. overlapping aggregate load;store (>=128B) lowered to forward memcpy loop (NVPTXLowerAggrCopies).
8. ldu.global non-const align: unguarded cast<ConstantInt> in getTgtMemIntrinsic (~4799).
9. AsmPrinter: sub-byte vector global / splat / non-byte-divisor element overflow/assert (bufferAggregateConstVec ~1803).
10. AsmPrinter: large iN (N>64,N%8!=0) global drops high partial byte (ExtendBuffer ~1737).
11. NVVMIntrRange maxclusterrank=UINT32_MAX APInt overflow.
12. tcgen05.ld/.st 16x32bx2 i64 offset built as MVT::i32 (SelectTcgen05Ld/St).
`

const BAR = `
You are hunting NEW CORRECTNESS BUGS in the LLVM NVPTX backend. Be thorough and go DEEP in your assigned area.

WHAT COUNTS (priority): (1) MISCOMPILE — emitted PTX/MIR computes a different result than the input IR for a well-defined (non-UB) input; (2) compiler SEGFAULT/OOB/UAF reachable from valid input; (3) (low value) assertion failures from valid IR.

WHAT DOESN'T COUNT: "cannot select"/unsupported/report_fatal_error on genuinely unsupported ops; dropped metadata; missed optimizations; perf; style; an assertion that degrades to a graceful report_fatal_error in release.

${KNOWN}

RIGOR: For a miscompile, identify a SPECIFIC defined input and the exact wrong result, tracing the code; for a crash, the exact valid input. You HAVE a built NVPTX compiler at ${LLC} — USE IT to confirm before reporting (set confirmed_with_llc). Cross-check "correct" answers against another target (e.g. \`llc -mtriple=x86_64\`) or \`opt\` constant-folding when useful. Prefer a few well-substantiated findings; an empty array is fine if your area is clean. Quality over quantity, but be exhaustive in coverage of your area.

For each finding: title; file+lines; kind (miscompile|segfault|assertion|other); mechanism (code excerpt + why wrong); trigger; ir (minimal self-contained LLVM IR for llc -mtriple=nvptx64); llc_cmd; confidence (0-1); confirmed_with_llc (bool).
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
        llc_cmd: { type: 'string' }, confidence: { type: 'number' }, confirmed_with_llc: { type: 'boolean' },
      },
      required: ['title', 'file', 'lines', 'kind', 'mechanism', 'trigger', 'ir', 'llc_cmd', 'confidence', 'confirmed_with_llc'],
    } },
  },
  required: ['findings'],
}

const TARGETS = [
  // ---- getTgtMemIntrinsic full table, finer ----
  { key: 'T01-tgtmem-A', loc: `${SRC}/NVPTXISelLowering.cpp lines 4266-4900`,
    focus: 'getTgtMemIntrinsic entries (atomics, ld/st, cp.async, wmma, ldu/ldg, tex/surf). For EACH entry check: is memVT the true access size/type? Is MOLoad/MOStore/MOVolatile correct (a store intrinsic must be MOStore, etc.)? Is align read from a genuinely-ImmArg operand via cast<ConstantInt> (else crash, like the known ldu bug — but find DIFFERENT intrinsics with the same unguarded cast on a NON-ImmArg operand)? Report wrong-size memVT only if it can cause a real reorder/misload, and unguarded casts on non-ImmArg operands.' },
  { key: 'T02-tgtmem-B', loc: `${SRC}/NVPTXISelLowering.cpp lines 4900-5602`,
    focus: 'Same as T01 for the rest of getTgtMemIntrinsic. Cross-reference operand ImmArg-ness in include/llvm/IR/IntrinsicsNVVM.td. Find unguarded cast<ConstantInt>/cast<ConstantSDNode> on non-ImmArg intrinsic operands, and MOLoad/MOStore/size mistakes.' },
  // ---- knownbits / demanded bits ----
  { key: 'T03-knownbits-demanded', loc: `${SRC}/NVPTXISelLowering.cpp lines 7747-7863 plus simplifyDemandedBitsForPRMT/canonicalizePRMTInput 7792-7844`,
    focus: 'computeKnownBitsForTargetNode and SimplifyDemandedBitsForTargetNode: do they OVER-claim known bits (claim a bit is 0/1 when it can be otherwise) for PRMT/MUL_WIDE/etc.? Over-claimed known bits let generic DAGCombine delete needed code -> miscompile. Construct IR where the over-claim changes output.' },
  // ---- ReplaceNodeResults ----
  { key: 'T04-replacenoderesults', loc: `${SRC}/NVPTXISelLowering.cpp lines 7438-7553 (ReplaceNodeResults)`,
    focus: 'Result legalization for illegal-typed nodes (i1, i128, v2/v4 vectors, f16/bf16). Look for wrong sign/zero extension, wrong element order when splitting/widening, bitcast width mistakes, dropping a chain. Build IR that returns/uses the illegal type.' },
  // ---- call / args extension ----
  { key: 'T05-lowercall-ext', loc: `${SRC}/NVPTXISelLowering.cpp lines 1365-1783 (correctParamType, LowerCall)`,
    focus: 'Argument/return marshalling: small-int args/returns sign- vs zero-extended per signext/zeroext; byval offset/align; vararg float promotion (f32->f64?); struct-by-value packing; i1 args. Find a case where the emitted extension/packing differs from IR (e.g. a signext i8 return zero-extended, or a vararg float not promoted to double matching the callee read).' },
  { key: 'T06-formalargs-ext', loc: `${SRC}/NVPTXISelLowering.cpp lines 4037-4253 (splitValueIntoRegisterParts, LowerFormalArguments)`,
    focus: 'Incoming param extension/assembly: signext/zeroext small ints, i1, 128-bit split hi/lo, vector params, aggregate params. Find wrong extension or wrong part order vs IR/ABI. Cross-check against the caller side.' },
  // ---- FMA / FADD / scalarize ----
  { key: 'T07-fma-fadd', loc: `${SRC}/NVPTXISelLowering.cpp lines 5697-6291 (allowFMA, performFADDCombineWithOperands, mayFoldFMULIntoFMA, performScalarizeV2F32Op, PerformFMinMaxCombine)`,
    focus: 'FMA contraction firing without contract/fast (changes rounding -> result differs from unfused IR); v2f32 scalarize/recombine lane errors; fmin/fmax NaN & signed-zero (minnum vs minimum vs maximumnum). Find a defined fp input where the result differs.' },
  // ---- PRMT family ----
  { key: 'T08-prmt', loc: `${SRC}/NVPTXISelLowering.cpp getPRMT 1891-1911, lowerPrmtIntrinsic 3005-3149, combinePRMT 6937-6963`,
    focus: 'PRMT byte-selector computation (each nibble selects a source byte / sign-replicate). bswap via PRMT (lowerBSWAP 2605) selector for i16/i32/i64. Find a selector constant that picks the wrong byte for a concrete input.' },
  // ---- bitwise: bfe/bfi/ctlz/ctpop/brev ----
  { key: 'T09-bfe-bfi-bit', loc: `${SRC}/NVPTXISelDAGToDAG.cpp tryBFE 1502-1714; ${SRC}/NVPTXInstrInfo.td bfe/bfi/brev/ctlz/popc patterns`,
    focus: 'Bit-field-extract start/width/signedness (SExt vs ZExt, off-by-one width, start>=width); BFI insert position/width; brev/popc/clz result width. Construct IR (and/shl/lshr/ashr that become bfe) where the extracted/insert value is wrong.' },
  // ---- min/max/abs/copysign ----
  { key: 'T10-minmax-abs-copysign', loc: `${SRC}/NVPTXISelLowering.cpp LowerFCOPYSIGN 2333; min/max/abs setOperationAction + patterns in NVPTXInstrInfo.td/NVPTXIntrinsics.td`,
    focus: 'integer min/max signedness (min.s vs min.u); fabs/fneg on f16/bf16 vectors; copysign with type mismatch; abs of INT_MIN; smax/umax selection. nvvm fmin/fmax NaN propagation (commit lowered nvvm.fmax to maximumnum). Find a defined input with wrong result.' },
  // ---- cvt rounding / sat / ftz patterns ----
  { key: 'T11-cvt-patterns', loc: `${SRC}/NVPTXInstrInfo.td cvt/conversion patterns (search 'cvt', 'fp_to', 'sint_to', 'uint_to', 'fpround', 'fpextend', 'sat')`,
    focus: 'Conversion patterns mapping ISD conversions to PTX cvt: wrong rounding mode (rn/rz/rm/rp), missing/incorrect .ftz, .sat applied or omitted incorrectly, signed vs unsigned cvt, f16/bf16/f32/f64/i8/i16/i32/i64 combos. Find a pattern whose PTX cvt qualifiers give a different value than the IR conversion for a concrete input. (Skip the known float->i1 one.)' },
  // ---- shift/rotate td + lowering edges ----
  { key: 'T12-shift-rotate', loc: `${SRC}/NVPTXISelLowering.cpp LowerShift*Parts 2215-2329, lowerFSH/expandFSH64/lowerROT 3225-3280; ${SRC}/NVPTXInstrInfo.td shift/rotate patterns`,
    focus: 'funnel shift (fshl/fshr) amount mod width vs clamp; rotate by 0 or width; shift amount masking; SHL/SRL/SRA patterns by constant vs reg; the 32-bit-amount truncation for non-i64 wide types. Find a defined input where the shift/rotate result is wrong (DIFFERENT from the known i64 clamp bug).' },
  // ---- load/store selection deep ----
  { key: 'T13-load-store-sel', loc: `${SRC}/NVPTXISelDAGToDAG.cpp tryLoad/tryLoadVector/tryStore/tryStoreVector 1111-1502`,
    focus: 'Extending-load fromType (sext vs zext vs anyext) chosen correctly? Vector element order in v2/v4 load/store? Volatile/atomic ordering & scope preserved? Alignment-based opcode picking an unaligned access as aligned? i8/i16 sub-register handling. Find wrong extension or lost volatile/order.' },
  // ---- LDG/LDU invariance ----
  { key: 'T14-ldg-ldu', loc: `${SRC}/NVPTXISelDAGToDAG.cpp tryLDG 1266-1342, tryLDU 1342-1383`,
    focus: 'ld.global.nc (LDG) / ldu require the memory to be invariant/read-only for the kernel duration. Is LDG/LDU ever emitted for a load that is NOT provably invariant (e.g. a plain global load that could alias a store)? That would read stale data. Check the guards.' },
  // ---- addrspacecast ----
  { key: 'T15-addrspacecast', loc: `${SRC}/NVPTXISelDAGToDAG.cpp SelectAddrSpaceCast 927-1105; ${SRC}/NVPTXISelLowering.cpp LowerADDRSPACECAST 3542, combineADDRSPACECAST 6866`,
    focus: 'cvta direction (to-generic vs from-generic), correct address-space numbers (global=1,shared=3,const=4,local=5,param), null pointer handling per AS (a null generic ptr must map correctly), folding addrspacecast chains. Find a cast that targets the wrong space or mishandles null.' },
  // ---- atomics / fence ----
  { key: 'T16-atomic-fence', loc: `${SRC}/NVPTXISelDAGToDAG.cpp selectAtomicSwap128 2261, tryFence 1855; ${SRC}/NVPTXISelLowering.cpp shouldInsertFencesForAtomic 7553; ${SRC}/NVPTXIntrinsics.td atom patterns; ${SRC}/NVPTXAtomicLower.cpp`,
    focus: 'atomic memory-ordering/scope qualifier mapping (acquire/release/acq_rel/seq_cst; .cta/.gpu/.sys); missing fence for seq_cst; atom op mapped to wrong operation (add/min/max/and/or/xor/exch); atom.min/max signedness (s vs u); 128-bit atomic hi/lo order. Find a semantic mismatch.' },
  // ---- vector lane ordering ----
  { key: 'T17-vector-lanes', loc: `${SRC}/NVPTXISelLowering.cpp LowerBUILD_VECTOR/EXTRACT/INSERT/SHUFFLE 2052-2215, lowerLoadVector/lowerSTOREVector 3793-3968, combinePackingMovIntoStore 5966; ${SRC}/NVPTXISelDAGToDAG.cpp tryUNPACK_VECTOR/tryEXTRACT_VECTOR_ELEMENT 435-927, SelectV2I64toI128/SelectI128toV2I64 1801-1855`,
    focus: 'v2f16/v2bf16/v2i16/v4i8 lane packing order (hi/lo swap), extractelement picking wrong lane, shuffle mask index, 128-bit<->2xi64 hi/lo order, packing mov into store writing lanes swapped. Use extractelement + llc to confirm a lane swap.' },
  // ---- under-explored files ----
  { key: 'T18-image-handles', loc: `${SRC}/NVPTXReplaceImageHandles.cpp (whole, 74KB)`,
    focus: 'Replacing image/sampler handles with the right surface/texture operand. Look for wrong operand index, off-by-one in handle->index mapping, a switch missing cases that silently passes through a wrong handle, signedness/size of the replaced immediate. This file is large and under-audited.' },
  { key: 'T19-proxyreg-peephole-imageopt', loc: `${SRC}/NVPTXProxyRegErasure.cpp, ${SRC}/NVPTXPeephole.cpp, ${SRC}/NVPTXImageOptimizer.cpp`,
    focus: 'ProxyRegErasure: erasing PROXY_REG without correctly forwarding the value (wrong replacement reg). NVPTXPeephole: machine peephole (e.g. cvta+load fusion) that changes address space/semantics. ImageOptimizer: folding image query to a constant that may be wrong. Find a transform that changes results.' },
  { key: 'T20-forwardparams-setbyval-markptrs', loc: `${SRC}/NVPTXForwardParams.cpp, ${SRC}/NVPTXSetByValParamAlign.cpp, ${SRC}/NVPTXMarkKernelPtrsGlobal.cpp, ${SRC}/NVPTXLowerAlloca.cpp`,
    focus: 'ForwardParams: forwarding a param-space pointer where a generic/local copy is required (aliasing/lifetime). SetByValParamAlign: computing a wrong (too large) alignment that misaligns accesses. MarkKernelPtrsGlobal: marking a pointer global when it could be another space. LowerAlloca: alloca address-space/cast errors. Find a correctness mismatch.' },
  // ---- MC layer ----
  { key: 'T21-mc-instprinter', loc: `${SRC}/MCTargetDesc/ (InstPrinter, NVPTXMCExpr.cpp, NVPTXInstPrinter.cpp, AsmBackend, NVPTXMCAsmInfo)`,
    focus: 'InstPrinter operand/modifier printing: a modifier method that prints the wrong thing (wrong sign, wrong suffix, wrong register, dropped negation), MCExpr evaluation, float/immediate printing. A misprinted operand is a miscompile of the text PTX. Check printMemOperand, printLdStCode/CvtMode/CmpMode modifier printers against their enum semantics.' },
  // ---- branch analysis / instrinfo ----
  { key: 'T22-instrinfo-branch', loc: `${SRC}/NVPTXInstrInfo.cpp (analyzeBranch, insertBranch, removeBranch, reverseBranchCondition, copyPhysReg, isLoadFromStackSlot/StoreToStackSlot) and the CBranch inverted-flag handling`,
    focus: 'reverseBranchCondition inverting the wrong sense; analyzeBranch misreporting TrueBB/FalseBB/fallthrough or the condition operands -> wrong CFG after if-conversion/block reordering; copyPhysReg emitting a wrong-width/type move; CBranch inverted flag (recent feature) mishandled. Construct a multi-block function exercising branch folding.' },
  // ---- inline asm ----
  { key: 'T23-inline-asm', loc: `${SRC}/NVPTXISelLowering.cpp LowerAsmOperandForConstraint 4253, getRegForInlineAsmConstraint, getConstraintType; ${SRC}/NVPTXISelDAGToDAG.cpp SelectInlineAsmMemoryOperand 1785`,
    focus: 'Inline-asm constraint -> register class mapping: a constraint that picks the wrong-width register class (e.g. 32-bit class for a 64-bit value), memory operand selection, immediate constraint range. Wrong reg class corrupts the value. Construct call asm IR.' },
  // ---- intrinsics.td atomics/warp ----
  { key: 'T24-intrinsics-atom-warp', loc: `${SRC}/NVPTXIntrinsics.td (atom.*, red.*, shfl.*, vote.*, match.*, redux.*, dp4a/dp2a, mma) patterns`,
    focus: 'atom/red op signedness & operation (min/max .s/.u, inc/dec wrap value, cas operand order); shfl mode (up/down/bfly/idx) & clamp/mask packing; vote mode (all/any/ballot/uni); redux op & abs; dp4a/dp2a signedness; mbarrier. Find a pattern mapping the intrinsic to the wrong PTX op/qualifier.' },
  // ---- reqntid/clusterdim constant folding ----
  { key: 'T25-sreg-constfold', loc: `${SRC}/NVVMIntrRange.cpp, ${SRC}/NVPTXISelLowering.cpp or wherever blockDim/clusterDim are constant-folded when reqntid/reqnctapercluster/maxntid are set (recent commits 058398c, 3fdbee1)`,
    focus: 'Constant-folding tid/ntid/clusterid/nctaid sreg reads to constants based on reqntid/maxntid/cluster_dim attributes. Is the folded constant correct (e.g. folding ntid.x to reqntid.x is valid, but folding tid.x or using maxntid as if it were reqntid would be wrong)? An attribute that is an UPPER BOUND (maxntid) must NOT be folded to an exact value. Find a wrong fold or wrong !range.' },
]

phase('DeepSweep')

const results = await parallel(TARGETS.map(t => () =>
  agent(`${BAR}\n\n=== YOUR DEEP-DIVE AREA ===\nRead and analyze: ${t.loc}\n\nScrutinize specifically: ${t.focus}\n\nRead the real source (Read/Grep) and confirm with the built llc. Return findings via structured output.`,
    { label: t.key, phase: 'DeepSweep', schema: SCHEMA })
    .then(r => ({ key: t.key, findings: (r && r.findings) || [] }))
    .catch(() => ({ key: t.key, findings: [] }))
))

const all = []
for (const r of results) for (const f of (r.findings || [])) all.push({ region: r.key, ...f })
all.sort((a, b) => (b.confidence || 0) - (a.confidence || 0))
log(`Round3: ${all.length} raw findings across ${TARGETS.length} deep areas`)
return { count: all.length, findings: all }
