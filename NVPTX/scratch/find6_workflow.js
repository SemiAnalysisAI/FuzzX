export const meta = {
  name: 'nvptx-find-round6',
  description: 'Round 6: broad sweep of NVPTX intrinsic patterns, feature predicates, immediate widths, machine passes',
  phases: [{ title: 'Sweep6' }],
}

const SRC = '/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX'
const INC = '/Users/justinlebar/code/vm-shared/llvm/llvm/include/llvm/IR'
const LLC = '/Users/justinlebar/code/llvm2/build/bin/llc'
const README = '/Users/justinlebar/code/FuzzX/NVPTX/README.md'

const BAR = `
You are hunting NEW CORRECTNESS BUGS in the LLVM NVPTX backend. Go DEEP in your assigned area; be exhaustive.

WHAT COUNTS: (1) MISCOMPILE (emitted PTX/MIR != input IR for a well-defined non-UB input); (2) compiler SEGFAULT/OOB/UAF/assert from valid input; (3) the backend emitting INVALID/unassemblable PTX for valid IR — a non-existent type/qualifier, or an instruction NOT supported by the declared -mcpu/-mattr target (ptxas would reject) — report as kind 'other'.
WHAT DOESN'T: cannot-select/unsupported on genuinely unsupported ops; dropped metadata; missed-opt; perf; style; assert that degrades to a graceful report_fatal_error in release; bugs only under UB/poison; spec-lawyering with no observable PTX difference. VERIFY PTX semantics against the actual PTX ISA, not assumptions (e.g. PTX min/max DO order signed zeros; .ftz on a cvt only affects f32 operands).

EXCLUDE already-found bugs: FIRST read ${README} (the catalog of 33 already-confirmed bugs + a "rejected/not-a-bug" list). Do NOT re-report anything already in that file. Find DIFFERENT bugs.

RIGOR: specific defined input + exact wrong result/crash, tracing the code. USE the built llc at ${LLC} to confirm (set confirmed_with_llc); cross-check "correct" answers via \`llc -mtriple=x86_64\` or \`opt\` folding. For "invalid PTX on wrong arch" claims, name the exact PTX ISA / sm version the instruction requires and show the emitted .target is lower. Empty array is fine. Quality over quantity.

Per finding: title; file+lines; kind (miscompile|segfault|assertion|other); mechanism (code excerpt + why wrong); trigger; ir; llc_cmd; confidence; confirmed_with_llc.
`

const SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: { findings: { type: 'array', items: {
    type: 'object', additionalProperties: false,
    properties: {
      title: { type: 'string' }, file: { type: 'string' }, lines: { type: 'string' },
      kind: { type: 'string', enum: ['miscompile', 'segfault', 'assertion', 'other'] },
      mechanism: { type: 'string' }, trigger: { type: 'string' }, ir: { type: 'string' },
      llc_cmd: { type: 'string' }, confidence: { type: 'number' }, confirmed_with_llc: { type: 'boolean' },
    },
    required: ['title', 'file', 'lines', 'kind', 'mechanism', 'trigger', 'ir', 'llc_cmd', 'confidence', 'confirmed_with_llc'],
  } } },
  required: ['findings'],
}

const TARGETS = [
  { key: 'W01-wmma-mma-ldmatrix', loc: `${SRC}/NVPTXIntrinsics.td (wmma.*, mma.*, ldmatrix.*, stmatrix.*, movmatrix) + ${INC}/IntrinsicsNVVM.td`,
    focus: 'wmma/mma/ldmatrix fragment element types, counts, layout (row/col), and the memVT in getTgtMemIntrinsic. A fragment pattern mapping to the wrong element type/count, a load/store fragment with wrong size, ldmatrix .trans flag, stmatrix lane mapping. Find a pattern emitting the wrong instruction/qualifier vs the intrinsic contract.' },
  { key: 'W02-cpasync-bulk', loc: `${SRC}/NVPTXIntrinsics.td (cp.async.*, cp.async.bulk.*, cp.async.bulk.tensor.*) + ${SRC}/NVPTXISelDAGToDAG.cpp SelectCpAsyncBulk*`,
    focus: 'cp.async / cp.async.bulk(.tensor) operand ORDER, tensor dims count, im2col offsets, cache-hint and multicast operand placement, immediate-width truncation (built as i32 vs i64), the mbarrier operand. Find an operand placed wrong or an immediate truncated.' },
  { key: 'W03-tcgen05-rest', loc: `${SRC}/NVPTXISelDAGToDAG.cpp (SelectTcgen05* beyond ld/st offset), ${SRC}/NVPTXISelLowering.cpp lowerTcgen05St/lowerTcgen05MMADisableOutputLane, ${SRC}/NVPTXIntrinsics.td tcgen05.*`,
    focus: 'tcgen05 alloc/dealloc/relinquish/commit/wait/mma/cp/shift/ld/st: immediate widths (CtaGroup, etc. built with getTargetConstant MVT::i32 where wider), operand ordering, the DisableOutputLane mask computation, shape/scale handling. Find a truncated immediate or misplaced operand (other than the known ld/st 16x32bx2 offset).' },
  { key: 'W04-tex-surf', loc: `${SRC}/NVPTXIntrinsics.td (tex.*, tld4.*, suld.*, sust.*, sured.*, txq/suq) + ${SRC}/NVPTXReplaceImageHandles.cpp`,
    focus: 'texture/surface: coordinate count per dimensionality (1d/2d/3d/a1d/a2d/cube), result vector size (v4), surface load/store clamp mode (.trap/.clamp/.zero), signedness of suld result type (.s8/.u8/.b8), sured op. Find a pattern with wrong coord count, wrong vector width, wrong clamp, or wrong signedness vs the intrinsic.' },
  { key: 'W05-warp-shfl-vote', loc: `${SRC}/NVPTXIntrinsics.td (shfl.sync.*, vote.*, vote.sync.*, match.*, redux.sync.*, activemask, bar.*, barrier.*)`,
    focus: 'shfl mode (up/down/bfly/idx) and the packed control operand (clamp<<8|segmask) bit layout; vote/vote.sync mode and result type; match.any/all.sync; redux.sync op (.add/.min/.max/.and/.or/.xor) and signedness/.abs; bar/barrier.sync operands. Find a wrong mode/qualifier/operand-pack.' },
  { key: 'W06-math-approx', loc: `${SRC}/NVPTXIntrinsics.td + ${SRC}/NVPTXInstrInfo.td (sin/cos/ex2/lg2/rcp/rsqrt/sqrt/div/tanh approx; rn/rz/rm/rp; ftz)`,
    focus: 'math intrinsic -> PTX: wrong rounding suffix, missing/extra .ftz, .approx where full-precision required (or vice versa), div.rn vs div.approx vs div.full, rcp/rsqrt approx predicate. Find a case whose emitted qualifiers give a value DIFFERENT from what the intrinsic mandates for a concrete input (not just precision-within-tolerance).' },
  { key: 'W07-mad-dp4a-mul24', loc: `${SRC}/NVPTXInstrInfo.td + ${SRC}/NVPTXIntrinsics.td (mad.lo/hi/wide, mul24/mad24, sad, dp4a/dp2a, fns, bfind, bmsk)`,
    focus: 'integer mad/mul24/mad24 signedness (s/u) + 24-bit truncation; mad.hi vs lo; sad accumulate; dp4a/dp2a per-element signedness; fns/bfind/bmsk operands. Find a pattern computing a different value than the matched node/intrinsic for a concrete input.' },
  { key: 'W08-ld-st-intrinsics', loc: `${SRC}/NVPTXIntrinsics.td (ld.global.nc, ldu, prefetch, isspacep, mapa, cvta.*) + getTgtMemIntrinsic entries for them`,
    focus: 'ld.global.nc/ldu invariance safety; prefetch space; isspacep predicate correctness; mapa/cvta address-space mapping; the memVT/flags for these. Find an unsafe invariance assumption, wrong AS, or wrong predicate.' },
  { key: 'W09-atom-red-full', loc: `${SRC}/NVPTXIntrinsics.td (ALL ATOM*/RED* multiclasses 2486-2760) + ${INC}/IntrinsicsNVVM.td`,
    focus: 'FULL atom/red re-sweep beyond min/max-signed, .noftz, cas: atom.inc/dec wrap-value, exch, and/or/xor types, vector atom (f16x2/bf16x2 add), red.* (no-return) op correctness, system vs gpu vs cta scope mapping, f32/f64 scoped add qualifiers, the generic (unscoped) atomicrmw lowering. Each: emitted PTX op/qualifier vs intrinsic contract.' },
  { key: 'W10-cvt-narrowfp', loc: `${SRC}/NVPTXIntrinsics.td + ${SRC}/NVPTXInstrInfo.td (cvt to/from e4m3/e5m2 fp8, e2m1 fp4, e3m2/e2m3 fp6, tf32, bf16x2; cvt rounding/sat)`,
    focus: 'narrow-fp conversions: rounding mode (.rn/.rz), saturation (.satfinite), .relu, the x2/x4 packing, signedness. Find a conversion intrinsic mapped to wrong cvt qualifiers or wrong packing that changes the value for a concrete input.' },
  { key: 'W11-f16x2-vec-arith', loc: `${SRC}/NVPTXInstrInfo.td + ${SRC}/NVPTXISelLowering.cpp (v2f16/v2bf16/v2i16 add/sub/mul/fma/min/max/neg/abs/setp patterns)`,
    focus: 'packed half2/bf16x2 binops: a pattern swapping the two lanes, wrong lane pairing, fma operand order, neg/abs on packed, setp.f16x2 predicate-pair order, the scalarize-then-repack path. Use extractelement + llc to confirm a per-lane wrong result.' },
  { key: 'W12-barrier-fence-cluster', loc: `${SRC}/NVPTXIntrinsics.td (mbarrier.*, fence.*, griddepcontrol, clusterlaunchcontrol, cluster.*, elect.sync) + tryFence`,
    focus: 'mbarrier arrive/expect/try_wait operand & count; fence proxy/scope/ordering; griddepcontrol; clusterlaunchcontrol try_cancel/query_cancel operand handling; cluster map/rank. Find a wrong scope/ordering/operand.' },
  { key: 'W13-feature-predicate-sweep', loc: `${SRC}/NVPTXInstrInfo.td + ${SRC}/NVPTXIntrinsics.td — grep Requires<[...]> and find instructions/patterns with MISSING or too-low predicates`,
    focus: 'Like the scoped-CAS bug: find instructions/patterns that emit a PTX instruction/qualifier requiring sm_XX/PTX_Y but whose Pat/instr has NO (or too-low) Requires<[hasSM<>/hasPTX<>]>. Cross-check the instruction real min arch (PTX ISA) vs the predicate. Compile at a LOW -mcpu (sm_50/sm_60/sm_70) and check whether an instruction invalid for that .target is emitted with no error. Enumerate several.' },
  { key: 'W14-gettgtmem-resweep2', loc: `${SRC}/NVPTXISelLowering.cpp getTgtMemIntrinsic 4266-5602`,
    focus: 'Yet another pass for: unguarded cast<ConstantInt>/cast<ConstantSDNode> on a NON-ImmArg operand (crash on runtime value) — DIFFERENT intrinsics than ldu.global; a store/atomic intrinsic whose flags omit MOStore (enabling DSE/reorder) or a load marked MOStore; memVT wrong size affecting correctness. List intrinsic + ImmArg status from IntrinsicsNVVM.td.' },
  { key: 'W15-dagtodag-imm-width', loc: `${SRC}/NVPTXISelDAGToDAG.cpp (all getTargetConstant/getConstant calls building instruction immediates; Select* routines)`,
    focus: 'Sweep every getTargetConstant(x.getZExtValue(), DL, MVT::iNN) / CurDAG->getTargetConstant in selection for cases where the source value/operand is WIDER than the chosen MVT (high bits dropped) — like the tcgen05 offset bug but in other Select* routines (cp.async, prefetch, fence scope, shfl, etc.). Cross-check the instruction field width in the .td.' },
  { key: 'W16-replaceimagehandle-deep', loc: `${SRC}/NVPTXReplaceImageHandles.cpp (whole file, the big tables findOptimalImageHandle / replaceImageHandle and the per-opcode operand-index tables)`,
    focus: 'Beyond the select->SELP crash: wrong operand INDEX in the per-opcode tables (reads/replaces the wrong operand), an off-by-one in a handle->image-index map, a switch missing an instruction that silently passes a wrong handle, signedness/size of a replaced immediate. This 74KB file is under-audited.' },
  { key: 'W17-proxyreg-peephole', loc: `${SRC}/NVPTXProxyRegErasure.cpp, ${SRC}/NVPTXPeephole.cpp, ${SRC}/NVPTXImageOptimizer.cpp`,
    focus: 'ProxyRegErasure forwarding the wrong replacement reg; NVPTXPeephole machine peephole (cvta+ld/st fusion, frame-index folding) that changes address space or semantics; ImageOptimizer folding an image query (channel order/data type/width) to a wrong constant. Find a transform changing results.' },
  { key: 'W18-genericnvvm-alloca', loc: `${SRC}/NVPTXGenericToNVVM.cpp, ${SRC}/NVPTXLowerAlloca.cpp, ${SRC}/NVPTXAllocaHoisting.cpp, ${SRC}/NVPTXMarkKernelPtrsGlobal.cpp`,
    focus: 'GenericToNVVM rewriting global var address spaces and the cvta inserted for uses (wrong AS, dropped initializer, wrong handling of a global used in a constant-expr); LowerAlloca AS/cast; AllocaHoisting moving an alloca past a point that changes lifetime; MarkKernelPtrsGlobal mismarking. Find a correctness mismatch.' },
  { key: 'W19-dwarf-debug', loc: `${SRC}/NVPTXDwarfDebug.cpp, ${SRC}/NVPTXAsmPrinter.cpp debug paths (.loc, inlined .loc, DW_AT emission)`,
    focus: 'Debug-info emission CRASHES on valid input (the user does not care about wrong line info, but a crash counts): null deref on a missing scope/file, an assert on an inlined-at chain, a bad cast. Recently changed (DW_AT_LLVM_language_dialect, .loc for inlined PTX). Find a crash from valid IR with debug info.' },
  { key: 'W20-callconv-return', loc: `${SRC}/NVPTXISelLowering.cpp (LowerReturn, LowerCall return extraction, LowerFormalArguments) + ${SRC}/NVPTXAsmPrinter.cpp param/retval decls`,
    focus: 'Calling convention: returning/passing a struct by value (field offsets, alignment), sret, multiple return values, byval alignment propagation (NVPTXSetByValParamAlign), vararg float promotion (f32->f64), i1/sub-byte return. Find a layout/extension mismatch between caller and callee or vs IR (distinct from #006/#021/#025).' },
  { key: 'W21-atomicrmw-expand', loc: `${SRC}/NVPTXISelLowering.cpp (shouldExpandAtomicRMWInIR, atomicrmw fsub/fmin/fmax/fminimum/fmaximum/nand handling, ATOMIC_LOAD_F* actions) + ${SRC}/NVPTXAtomicLower.cpp`,
    focus: 'atomicrmw beyond fadd-f32: fsub (negate-then-add sign), fmin/fmax (signed-zero/NaN, must they expand to a CAS loop? is the loop correct?), fmaximum/fminimum, nand, integer min/max width<32 (sign), i128 atomicrmw. Find an fp/int atomicrmw whose emitted code computes a different stored/returned value than IR for a concrete input.' },
  { key: 'W22-vector-ld-st-bitcast', loc: `${SRC}/NVPTXISelDAGToDAG.cpp tryLoadVector/tryStoreVector, ${SRC}/NVPTXISelLowering.cpp lowerLoadVector/lowerSTOREVector/LowerBITCAST, ReplaceNodeResults vector cases`,
    focus: 'Vector ld/st of v3/v5/v8/v16 and v4i8/v2i16; element-to-chunk grouping (ld.v2/ld.v4) and leftover element offset; element ORDER within ld.v4; bitcast vector<->scalar lane order; <N x i1> vector load. Find an element at the wrong offset/lane (use extractelement + llc).' },
  { key: 'W23-lower-intrinsic-handlers', loc: `${SRC}/NVPTXISelLowering.cpp static lower* handlers: lowerCvtRSIntrinsics 2950, lowerPrmtIntrinsic 3005, lowerIntrinsicVoid 2828, LowerClusterLaunchControlQueryCancel 2910, tensormap replace 2777-2828`,
    focus: 'These hand-written intrinsic lowerings: wrong selector/constant, operand order, the cvt-rs (stochastic rounding) rbits handling, tensormap replace elemtype/swizzle mode constants, clusterlaunchcontrol query mask/predicate extraction. Find a wrong constant/operand producing a different result.' },
  { key: 'W24-knownbits-demanded2', loc: `${SRC}/NVPTXISelLowering.cpp computeKnownBitsForTargetNode 7747, SimplifyDemandedBitsForTargetNode 7844, simplifyDemandedBitsForPRMT 7800, performScalarizeV2F32Op 6209`,
    focus: 'Target knownbits/demanded-bits: does computeKnownBitsForTargetNode OVER-claim known bits for any NVPTXISD node (MUL_WIDE, PRMT, FSHL_CLAMP, etc.), letting generic DAGCombine delete needed code? Does SimplifyDemandedBits drop a demanded bit that matters? Construct IR where the over-claim changes output.' },
  { key: 'W25-fp-round-extend-copysign', loc: `${SRC}/NVPTXISelLowering.cpp LowerFP_ROUND 2492, LowerFP_EXTEND 2526, LowerFROUND/32/64 2348-2440, LowerFCOPYSIGN 2333, PromoteBinOpToF32 2440`,
    focus: 'fptrunc/fpext (other than int->bf16): f64->f16 double rounding, fp128 paths, dropped chain; FROUND (round-half-away) sign-of-zero/large magnitude; copysign with mismatched magnitude/sign types; PromoteBinOpToF32 for f16/bf16 fdiv/fsub (double rounding). Find a concrete fp input with a wrong result.' },
  { key: 'W26-setcc-select-combines2', loc: `${SRC}/NVPTXISelLowering.cpp PerformSETCCCombine 6640, PerformVSELECTCombine 6772, PerformEXTRACTCombine 6667, combinePRMT 6937, PerformADDCombine 6076, PerformMULCombine 6613, combineMADConstOne 6544, combineMulSelectConstOne 6555`,
    focus: 'DAG combines (other than the known shift one): setcc predicate flip after operand commute; vselect lane mask; extract-from-build_vector wrong element; PRMT combine selector; x*(C+1)->mad off-by-one; mul-select-const. Find a fold changing the numeric result for a defined input.' },
]

phase('Sweep6')

const results = await parallel(TARGETS.map(t => () =>
  agent(`${BAR}\n\n=== YOUR AREA ===\nRead and analyze: ${t.loc}\n\nScrutinize: ${t.focus}\n\nRead ${README} first (exclusions), then the real source, and confirm with llc. Return findings via structured output.`,
    { label: t.key, phase: 'Sweep6', schema: SCHEMA })
    .then(r => ({ key: t.key, findings: (r && r.findings) || [] }))
    .catch(() => ({ key: t.key, findings: [] }))
))

const all = []
for (const r of results) for (const f of (r.findings || [])) all.push({ region: r.key, ...f })
all.sort((a, b) => (b.confidence || 0) - (a.confidence || 0))
log(`Round6: ${all.length} raw findings across ${TARGETS.length} areas`)
return { count: all.length, findings: all }
