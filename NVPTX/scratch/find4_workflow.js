export const meta = {
  name: 'nvptx-find-miscompiles-round4',
  description: 'Round 4: more NVPTX backend areas (TableGen patterns, AsmPrinter inits, math/atomic/warp intrinsics, reductions)',
  phases: [{ title: 'Sweep4' }],
}

const SRC = '/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX'
const INC = '/Users/justinlebar/code/vm-shared/llvm/llvm/include/llvm/IR'
const LLC = '/Users/justinlebar/code/llvm2/build/bin/llc'

const KNOWN = `
ALREADY-FOUND (do NOT re-report; find DIFFERENT bugs):
- float->i1 as setp.eq 0; combineMulWide sext(shl nsw bits-1) negative const; PerformSELECTShiftCombine i64/cross-width shift guard dropped; int->bf16 double-round; byval param icmp/freeze/atomicrmw/cmpxchg ArgUseChecker misclassify (crash + cvta.param miscompile); va_arg i8 stride; overlapping aggregate load/store forward copy; ldu.global non-const align cast<ConstantInt>; AsmPrinter sub-byte vector global/splat/non-divisor + large-iN-global high-byte drop; NVVMIntrRange maxclusterrank overflow; tcgen05.ld/.st offset i32 trunc.
- tryLDG OOB on invariant ATOMIC_LOAD; printHexu32imm prmt selector bit31 misprint; replaceImageHandle select->SELP unreachable; internal ptx_kernel byval over-align; inline-asm constraint ignores operand width (truncation/malformed); minimumnum/maximumnum bare PTX min/max signed-zero; <N x i1> param OOB; scoped atomic min/max always signed.
`

const BAR = `
You are hunting NEW CORRECTNESS BUGS in the LLVM NVPTX backend. Go DEEP in your area.

WHAT COUNTS: (1) MISCOMPILE (emitted PTX/MIR != input IR semantics for a well-defined non-UB input); (2) compiler SEGFAULT/OOB/UAF from valid input; (3) (low value) assertion from valid IR.
WHAT DOESN'T: cannot-select/unsupported/report_fatal_error on unsupported ops; dropped metadata; missed-opt; perf; style; assert that degrades to graceful report_fatal_error in release; bugs only under UB/poison; pure spec-lawyering with no observable PTX difference.

${KNOWN}

RIGOR: give a SPECIFIC defined input + exact wrong result (trace the code); for crashes the exact valid input. USE the built llc at ${LLC} to confirm (set confirmed_with_llc) and cross-check "correct" answers via \`llc -mtriple=x86_64\` or \`opt\` folding. Empty array is fine. Quality over quantity.

Per finding: title; file+lines; kind; mechanism (code excerpt + why wrong); trigger; ir (minimal, for llc -mtriple=nvptx64); llc_cmd; confidence; confirmed_with_llc.
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
  { key: 'U01-atom-intrinsics', loc: `${SRC}/NVPTXIntrinsics.td (atom.*/red.* multiclasses ATOM2*/ATOM3*, inc/dec, cas, add.f, exch) cross-ref ${INC}/IntrinsicsNVVM.td`,
    focus: 'Beyond the known scoped min/max-signedness bug: atom.inc/dec wrap-value semantics; cas (compare-and-swap) operand order (cmp vs new); atom.add.f16/bf16/f32/f64 type/rounding; scoped vs non-scoped op mismatches; red.* (reduction, no return) mapped to wrong op; any atom family that ISel-picks the wrong signed/unsigned or wrong operation due to a shared intrinsic. Confirm emitted PTX op matches the intrinsic.' },
  { key: 'U02-shfl-vote-warp', loc: `${SRC}/NVPTXIntrinsics.td (shfl.sync.*, vote.*, match.*, redux.sync.*, activemask, bar.*)`,
    focus: 'shfl mode (up/down/bfly/idx) + packed c operand (clamp<<8 | mask) bit layout; vote mode (all/any/ballot/uni) result type; match.any/all.sync; redux.sync op & .abs/.NaN; bar.warp.sync mask. Find a pattern that maps the intrinsic to the wrong PTX mode/qualifier or mispacks the immediate.' },
  { key: 'U03-math-intrinsics', loc: `${SRC}/NVPTXIntrinsics.td + ${SRC}/NVPTXInstrInfo.td (sin/cos/lg2/ex2/rcp/rsqrt/sqrt/div approx; rn/rz/rm/rp rounding variants; ftz variants; fma.rn)`,
    focus: 'math intrinsic -> PTX mapping: wrong rounding-mode suffix (rn/rz/rm/rp), wrong/missing .ftz, .approx where full precision required (or vice versa), div.rn vs div.approx, rcp/rsqrt approx selection gated on the wrong predicate. Find a case whose emitted qualifiers give a different value than the IR intrinsic mandates for a concrete input (not just precision-within-spec).' },
  { key: 'U04-mad-mul24-video', loc: `${SRC}/NVPTXInstrInfo.td (mad.lo/hi, mul24/mad24, sad, dp4a/dp2a, vadd/vsub/vmin/vmax video ops, bfind/brev/brev patterns)`,
    focus: 'integer mad/mul24/mad24 signedness (s vs u) and the 24-bit truncation semantics; sad accumulate; dp4a/dp2a element signedness; video min/max/add saturation & signedness; bfind .shiftamt; brev width. Find a pattern whose PTX op computes a different value than the matched ISD/intrinsic for a concrete input.' },
  { key: 'U05-setp-selp-cmp', loc: `${SRC}/NVPTXInstrInfo.td (setp/set/selp patterns; ISD::SETCC/SELECT_CC mapping; ordered vs unordered fp compares; signed vs unsigned int compares)`,
    focus: 'SETCC condition-code -> PTX setp suffix: ordered (setp.lt) vs unordered (setp.ltu) for the right ISD CC (SETOLT vs SETULT etc.); signed (.s) vs unsigned (.u) for int compares; eq/ne on NaN; selp operand order/predicate sense. Find a compare whose emitted setp gives the wrong boolean for a concrete input (esp. NaN or sign-boundary). (Skip the known float->i1.)' },
  { key: 'U06-asmprinter-global-init', loc: `${SRC}/NVPTXAsmPrinter.cpp (printModuleLevelGV, emitGlobals, bufferAggregateConstant for ConstantExpr/pointer/array-of-pointer/struct-with-padding, relocations, aliases)`,
    focus: 'Global initializers beyond plain ints/vectors: array/struct of POINTERS (relocation/symbol emission), ConstantExpr (gep/ptrtoint/inttoptr) in an initializer, struct field padding bytes, nested aggregates, addrspace of the initialized global vs pointee, zeroinitializer vs explicit, GlobalAlias. Find an initializer whose emitted bytes/symbols differ from the IR constant. (Skip sub-byte/large-int known bugs.)' },
  { key: 'U07-asmprinter-param-retval', loc: `${SRC}/NVPTXAsmPrinter.cpp (emitFunctionParamList, param/retval .param decls, alignment, vector/aggregate return layout) + ${SRC}/NVPTXISelLowering.cpp return lowering`,
    focus: 'Parameter and return-value .param layout: vector return packing order, aggregate return field offsets, alignment of params/retvals, i1/sub-byte param decls (distinct from the known <N x i1> store-offset bug — look at the .param SIZE decl), byval param size. Find a layout mismatch between caller and callee or vs IR.' },
  { key: 'U08-tti-instcombine', loc: `${SRC}/NVPTXTargetTransformInfo.cpp (instCombineIntrinsic, simplifyDemandedVectorEltsIntrinsic, and any intrinsic-folding helpers)`,
    focus: 'Folds of NVPTX intrinsics in InstCombine: a fold that rewrites an intrinsic to a constant/simpler form with the WRONG value (wrong identity element, ignores rounding/ftz, wrong lane, wrong predicate). Remember target intrinsic folds need -mtriple=nvptx64 in opt to fire. Construct IR + opt to show the fold produces a wrong result.' },
  { key: 'U09-vecreduce-tree', loc: `${SRC}/NVPTXISelLowering.cpp (LowerVECREDUCE 1993, buildTreeReduction 1911-1993, getExtractVectorizedValue 347)`,
    focus: 'Tree reduction of vector reductions: for VECREDUCE_FADD/FMUL (strict, non-reassoc) does it reassociate (illegal, changes fp result)? For VECREDUCE_FMAX/FMIN does it use the NaN/signed-zero-correct op? For integer reductions, correctness of the tree. Find an input where the reduction result differs from the sequential IR semantics. Check whether reassoc/fast is required and whether it is enforced.' },
  { key: 'U10-f16x2-vector-binop', loc: `${SRC}/NVPTXInstrInfo.td + ${SRC}/NVPTXISelLowering.cpp (v2f16/v2bf16/v2i16 add/mul/fma/min/max/neg/abs patterns, BFE/pack/unpack, setp.f16x2)`,
    focus: 'Packed half2/bf16x2 arithmetic: a binop pattern that swaps the two lanes, applies the op to the wrong lane pairing, or mishandles the high/low half; fma.rn.f16x2 operand order; neg/abs on packed; setp.*.f16x2 producing the predicate pair in the wrong order. Use extractelement to confirm a per-lane wrong result.' },
  { key: 'U11-nvvmreflect-reqntid', loc: `${SRC}/NVVMReflect.cpp, ${SRC}/NVVMIntrRange.cpp, ${SRC}/NVPTXISelLowering.cpp / wherever ntid/tid/nctaid/ctaid sreg reads are constant-folded from reqntid/maxntid/cluster_dim/reqnctapercluster`,
    focus: 'NVVMReflect folding __nvvm_reflect to a constant (wrong value for an arch flag?). Constant-folding sreg reads: folding ntid.x to reqntid.x is valid, but folding to MAXntid (an upper bound) as if exact, or folding tid.x, or a !range that excludes legal values, is a miscompile. Check each fold uses an EXACT attribute (reqntid/cluster_dim), not a bound (maxntid/maxclusterrank). Find a wrong fold or wrong range.' },
  { key: 'U12-idiv-irem', loc: `${SRC}/NVPTXInstrInfo.td + ${SRC}/NVPTXISelLowering.cpp (sdiv/udiv/srem/urem lowering & patterns, div by constant, i64 div)`,
    focus: 'Integer division/remainder: signed vs unsigned div/rem selection, INT_MIN/-1 overflow, div-by-power-of-2 to shift (signed needs rounding toward zero correction), rem sign of result. Find an input where the emitted div/rem differs from IR (e.g. signed div by 2 lowered as arithmetic shift without the negative-rounding fixup).' },
  { key: 'U13-select-vselect', loc: `${SRC}/NVPTXISelLowering.cpp lowerSELECT 3308, PerformVSELECTCombine 6772, PerformSELECTShiftCombine 6732 (other branches), PerformSETCCCombine 6640`,
    focus: 'SELECT/VSELECT lowering & combines (other than the known shift one): vselect lane mask correctness; select with f16x2/vector operands; setcc combine that flips a predicate after commuting operands; select->and/or boolean folds with wrong polarity. Find a defined input with a wrong selected value.' },
  { key: 'U14-stackalloc-frame', loc: `${SRC}/NVPTXISelLowering.cpp LowerDYNAMIC_STACKALLOC 1783, LowerSTACKSAVE/RESTORE 1823-1845; ${SRC}/NVPTXFrameLowering.cpp; ${SRC}/NVPTXPrologEpilogPass.cpp`,
    focus: 'dynamic alloca size/alignment rounding & address space of the returned pointer (local vs generic); stacksave/restore correctness; frame index resolution / local depot offset computation in prolog/epilog (wrong offset = wrong address). Find a wrong address/alignment.' },
  { key: 'U15-ctor-dtor-unreachable', loc: `${SRC}/NVPTXCtorDtorLowering.cpp, ${SRC}/NVPTXLowerUnreachable.cpp`,
    focus: 'Global ctor/dtor lowering: ordering (priority), whether dtors run, symbol naming/emission correctness. LowerUnreachable: replacing unreachable with trap/exit — does it ever drop a needed side effect or fall through incorrectly (e.g. a noreturn call followed by unreachable that gets mishandled)? Find a correctness issue.' },
  { key: 'U16-madconstone', loc: `${SRC}/NVPTXISelLowering.cpp matchMADConstOnePattern 6531, combineMADConstOne 6544, combineMulSelectConstOne 6555, PerformMULCombine 6613`,
    focus: 'x*(C+1) -> mad/x*C+x style rewrites and select*const+1: verify the rewritten constant and added operand are correct (off-by-one in C, wrong addend, signedness). Find a defined input where x*(C+1) folding yields a different product.' },
  { key: 'U17-gettgtmem-resweep', loc: `${SRC}/NVPTXISelLowering.cpp getTgtMemIntrinsic 4266-5602`,
    focus: 'Re-sweep specifically for: (a) unguarded cast<ConstantInt>/cast<ConstantSDNode> on an operand NOT marked ImmArg in IntrinsicsNVVM.td (crash on runtime operand) — find DIFFERENT intrinsics than ldu.global; (b) a STORE intrinsic whose Info.flags omits MOStore or sets MOLoad (or vice versa) enabling illegal reorder/DSE; (c) memVT smaller than the real access in a way that affects correctness. List each with the intrinsic name and the .td ImmArg status.' },
  { key: 'U18-cp-async-bulk', loc: `${SRC}/NVPTXISelDAGToDAG.cpp (SelectCpAsyncBulk*, tcgen05, mbarrier) + ${SRC}/NVPTXIntrinsics.td cp.async.bulk patterns`,
    focus: 'cp.async.bulk(.tensor) and tcgen05 selection: immediate width truncation (like the tcgen05 offset bug but other operands — masks, multicast, cache hints, im2col offsets); operand ordering; mbarrier arrive/expect operand. Find an immediate built with too-narrow MVT or an operand placed in the wrong position.' },
  { key: 'U19-fp-extend-round', loc: `${SRC}/NVPTXISelLowering.cpp LowerFP_ROUND 2492, LowerFP_EXTEND 2526, FROUND/FROUND32/FROUND64 2348-2440, expandRoundInexactToOdd usage`,
    focus: 'fptrunc/fpext between f16/bf16/f32/f64/fp128: double rounding (other than the known int->bf16), wrong intermediate, dropped chain; FROUND (round-half-away) sign-of-zero and large-magnitude correctness; f64->f16 direct vs via f32. Find a concrete fp input with a wrong rounded result.' },
  { key: 'U20-bitcast-copysign-fp', loc: `${SRC}/NVPTXISelLowering.cpp LowerBITCAST 2024, LowerFCOPYSIGN 2333, PromoteBinOpToF32 2440-2461`,
    focus: 'bitcast between vector/scalar of equal size (lane order / half packing); copysign when magnitude and sign types differ (f32 sign into f16 etc.); PromoteBinOpToF32 for f16/bf16 binops (fadd/fmul/fsub/fdiv promoted to f32 then rounded — double rounding for which ops? fdiv especially). Find a defined fp input with a wrong result.' },
]

phase('Sweep4')

const results = await parallel(TARGETS.map(t => () =>
  agent(`${BAR}\n\n=== YOUR AREA ===\nRead and analyze: ${t.loc}\n\nScrutinize: ${t.focus}\n\nRead the real source and confirm with llc. Return findings via structured output.`,
    { label: t.key, phase: 'Sweep4', schema: SCHEMA })
    .then(r => ({ key: t.key, findings: (r && r.findings) || [] }))
    .catch(() => ({ key: t.key, findings: [] }))
))

const all = []
for (const r of results) for (const f of (r.findings || [])) all.push({ region: r.key, ...f })
all.sort((a, b) => (b.confidence || 0) - (a.confidence || 0))
log(`Round4: ${all.length} raw findings across ${TARGETS.length} areas`)
return { count: all.length, findings: all }
