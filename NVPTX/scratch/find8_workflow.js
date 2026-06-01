export const meta = {
  name: 'nvptx-find-round8',
  description: 'Round 8: remaining crash/arch/immediate seams (AsmPrinter consts, selection/pass/MC crashes, warp/cpasync/f16 arch, .b128 .sys, immediate widths)',
  phases: [{ title: 'Sweep8' }],
}

const SRC = '/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX'
const INC = '/Users/justinlebar/code/vm-shared/llvm/llvm/include/llvm/IR'
const LLC = '/Users/justinlebar/code/llvm2/build/bin/llc'
const README = '/Users/justinlebar/code/FuzzX/NVPTX/README.md'

const BAR = `
You are hunting NEW CORRECTNESS BUGS in the LLVM NVPTX backend. Be EXHAUSTIVE; enumerate EVERY distinct instance.

WHAT COUNTS: (1) MISCOMPILE; (2) compiler SEGFAULT/OOB/assert/stack-overflow/fatal-error from valid input; (3) backend emits INVALID/unassemblable PTX for valid IR (instruction/qualifier/type/space the declared target rejects), report kind 'other'.
WHAT DOESN'T: clean 'Cannot select'/unsupported (that is FINE, not a bug); dropped metadata; missed-opt; perf; report_fatal_error that is a graceful diagnostic; UB-only.

EXCLUDE already-found: FIRST read ${README} (55 confirmed bugs + rejected list); do NOT re-report them. Especially already covered: half/bfloat & x86_fp80/ppc_fp128 & large-int-constexpr global crashes; ctordtor non-struct element; scalable byval; narrow-fp cvt sm_80 guard; mma m16n8k8/m16n8k32 under-guard; shared::cluster ld/st & cvta; atom.b128 .sys ptx83; cmpxchg/atom on const/param/local AS; store to const AS; non-sync vote; cvta.param; add/sub ftz.sat order; sust.p subword; ldu align; tcgen05 offset; knownbits-prmt recursion; kernel i65-i127 param; ArgUseChecker byval. Find DIFFERENT instances.

RIGOR: exact emitted instruction / crash + exact ISA version/space rule (cite it), shown emitted .target/.version is lower. USE ${LLC} (set confirmed_with_llc). Empty array fine.

Per finding: title; file+lines; kind; mechanism (code excerpt + why wrong + ISA cite); trigger; ir; llc_cmd; confidence; confirmed_with_llc.
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
  { key: 'X01-asmprinter-ptr-const', loc: `${SRC}/NVPTXAsmPrinter.cpp bufferLEByte/bufferAggregateConstant/printScalarConstant/emitGlobals for pointer constants`,
    focus: 'Global initializers involving pointers/ConstantExpr NOT yet covered: a vector-of-pointers global (<2 x ptr>), inttoptr in an initializer, select/icmp ConstantExpr, a GlobalAlias initializer, a global initialized to a function pointer, blockaddress, a ConstantExpr gep with negative/large offset. Find crashes or wrong emitted bytes. Enumerate.' },
  { key: 'X02-asmprinter-struct-pad', loc: `${SRC}/NVPTXAsmPrinter.cpp aggregate/struct/array emission, padding, alignment`,
    focus: 'Struct/array layout emission: packed struct trailing/inter-field padding bytes (wrong/stale), over-aligned global, a named opaque struct, a [0 x T] zero-length array, a struct whose alloc size != sum of fields, nested array-of-struct padding. Find a wrong byte count/layout vs the DataLayout (cross-check x86). Enumerate.' },
  { key: 'X03-selection-crash2', loc: `${SRC}/NVPTXISelDAGToDAG.cpp + ${SRC}/NVPTXISelLowering.cpp unchecked cast<>/getConstantOperandVal/switch-default reachable from valid IR (beyond the found ldu/tryLDG/image/unhandled-AS ones)`,
    focus: 'More selection crashes: unguarded cast<ConstantSDNode>/cast<ConstantFPSDNode> on a non-ImmArg intrinsic operand; getConstantOperandVal past the operand list for an atomic/vector node kind; a ReplaceNodeResults/LowerOperation path with an unhandled VT; tryLoadVector/tryStoreVector on an odd element count. Enumerate distinct ones.' },
  { key: 'X04-pass-crash2', loc: `${SRC}/NVPTXLowerAggrCopies.cpp, NVPTXLowerUnreachable.cpp, NVPTXForwardParams.cpp, NVPTXProxyRegErasure.cpp, NVPTXPeephole.cpp, NVPTXTagInvariantLoads.cpp, NVPTXImageOptimizer.cpp, NVPTXAtomicLower.cpp`,
    focus: 'Crashes in IR/MI passes on valid input not yet found: an unchecked cast, a visitor/switch default, iterator invalidation, an assumption a global/instr shape can violate, a null deref. Enumerate distinct ones.' },
  { key: 'X05-mc-instprinter-crash', loc: `${SRC}/MCTargetDesc/NVPTXInstPrinter.cpp (every print* modifier method), NVPTXMCExpr.cpp`,
    focus: 'InstPrinter modifier methods (printCvtMode/printCmpMode/printLdStCode/printMmaCode/printMemOperand/printProtoIdent/printHexu32imm etc.): an input value outside the method switch -> llvm_unreachable/assert, or a wrong printed token for a valid value. Construct IR reaching it. Enumerate.' },
  { key: 'X06-warp-arch2', loc: `${SRC}/NVPTXIntrinsics.td match.sync/redux.sync/activemask/elect.sync/bar.warp.sync/vote.sync + predicates`,
    focus: 'Warp ops emitted without/with-too-low arch guard (NOT non-sync vote, already found): match.any/all.sync (sm_70/ptx60), redux.sync (sm_80/ptx70), elect.sync (sm_90/ptx80), activemask (sm_62?/ptx62), bar.warp.sync. Find each emitted on a lower target. Enumerate.' },
  { key: 'X07-cpasync-mbarrier-arch2', loc: `${SRC}/NVPTXIntrinsics.td cp.async*/mbarrier*/fence*/cluster*/st.bulk/prefetch/applypriority/discard + predicates`,
    focus: 'cp.async (sm_80/ptx70), cp.async.bulk (sm_90/ptx80), mbarrier init/arrive/test_wait (sm_80/ptx70), fence.proxy.* (sm_90), cluster.map/rank (sm_90), prefetch/applypriority/discard (various). Find each emitted without proper arch guard on a lower -mcpu. Enumerate.' },
  { key: 'X08-f16-bf16-arith-arch2', loc: `${SRC}/NVPTXInstrInfo.td f16/bf16 arith & compare patterns + ${SRC}/NVPTXISelLowering.cpp set*Action for f16/bf16/v2f16/v2bf16`,
    focus: 'bf16 scalar/vector arith requires sm_80/ptx70; f16x2 fma/min/max have specific reqs. Find an f16/bf16 binop (add/mul/sub/fma/min/max/neg/abs/setp) emitted on a target lacking it (e.g. add.bf16 / min.bf16x2 / fma.rn.bf16 on sm_70). Enumerate.' },
  { key: 'X09-ldst-b128-sys-arch', loc: `${SRC}/NVPTXInstrInfo.td ld/st .b128 patterns + scope/sem qualifiers; ${SRC}/NVPTXISelLowering.cpp 128-bit load/store`,
    focus: 'Like atom.b128 .sys (found): does ld.b128/st.b128 with a memory ordering/scope (volatile/atomic 128-bit load-store) emit `.sys`/`.b128` forms on a PTX version below their introduction (ld/st .b128 = PTX 8.3/sm_90; .sys on .b128 = PTX 8.4)? Find under-guarded 128-bit ld/st scope/version. Enumerate.' },
  { key: 'X10-immediate-width2', loc: `${SRC}/NVPTXISelDAGToDAG.cpp + ${SRC}/NVPTXISelLowering.cpp all getTargetConstant/getConstant building instruction immediates from intrinsic operands`,
    focus: 'Immediate-width truncation (like tcgen05 offset): a getTargetConstant(op.getZExtValue(), MVT::iNN) where the intrinsic operand is wider, in cp.async.bulk (dims/mask/cachehint), prefetch, fence, shfl control pack, mbarrier count, st.bulk size, clusterlaunchcontrol. Check each Select* / lower* immediate. Enumerate.' },
  { key: 'X11-red-atom-as2', loc: `${SRC}/NVPTXISelDAGToDAG.cpp getAddrSpace + ${SRC}/MCTargetDesc/NVPTXInstPrinter.cpp printAtomicCode + ${SRC}/NVPTXIntrinsics.td atom/red`,
    focus: 'Atomic/AS combos NOT yet found (#040 const/param, #054 cmpxchg-local): atomicrmw on shared::cluster (AS7) emitting atom.shared::cluster without cluster guard; atom on AS that prints a malformed space; red reductions if any reach the printer; atom float on local. Enumerate distinct invalid-PTX combos.' },
  { key: 'X12-cvt-sat-relu-arch', loc: `${SRC}/NVPTXInstrInfo.td + ${SRC}/NVPTXIntrinsics.td cvt patterns with .sat/.relu/.ftz and narrow-int conversions`,
    focus: 'cvt qualifiers emitted on a target lacking them (.relu sm_80, .satfinite-on-some-types, tf32 sm_80, fp8/fp6/fp4 sm_89/sm_100), OR a wrong cvt modifier order (like add/sub ftz.sat), NOT the already-found f2bf16/f2f16/ff2*x2 ones. Look at cvt to/from i8/u8 with .sat, cvt.pack, cvt with rounding on integer. Enumerate.' },
  { key: 'X13-misc-miscompile2', loc: `${SRC}/NVPTXISelLowering.cpp lowering & combines; ${SRC}/NVPTXInstrInfo.td patterns`,
    focus: 'Keep hunting a TRUE VALUE-MISCOMPILE (highest value): a conversion/shift/select/combine/vector-lane op producing the wrong numeric result for a concrete defined input, confirmed via llc + x86/opt cross-check. Look hard at: fptosi/fptoui saturation to small ints, uitofp i64 rounding, signed div/rem by constant, bfe sign/width, select on vectors, prmt selector folds, v4i8 lane extract.' },
  { key: 'X14-misc-miscompile3', loc: `${SRC}/NVPTXISelLowering.cpp atomicrmw/copysign/abs/min-max/bitcast; ${SRC}/NVPTXInstrInfo.td`,
    focus: 'Another TRUE VALUE-MISCOMPILE hunt: fp atomicrmw fsub/fmin/fmax value, integer atomicrmw min/max width<32 sign, copysign type-mismatch, fabs/fneg on packed vectors, bitcast vector lane order, fminnum/fmaxnum NaN propagation vs PTX min/max. Confirm a wrong value numerically.' },
  { key: 'X15-tex-surf2', loc: `${SRC}/NVPTXIntrinsics.td tex/tld4/suld/sust/sured beyond sust.p subword + ${SRC}/NVPTXReplaceImageHandles.cpp`,
    focus: 'tex/surf NOT yet found: suld/sust unformatted with wrong .b<N> for the type, sured op/type, tld4 component, txq/suq query, array-texture coord count, wrong vector width in result, signedness of suld result. Also more replaceImageHandle unhandled-def cases. Enumerate.' },
  { key: 'X16-fp-conv-value', loc: `${SRC}/NVPTXISelLowering.cpp LowerFP_ROUND/FP_EXTEND/FROUND32/FROUND64/INT_TO_FP/FP_TO_INT + cvt patterns`,
    focus: 'A conversion VALUE-MISCOMPILE beyond float->i1 / int->bf16: f64->f16 double rounding, fptoui/fptosi saturation/overflow behavior vs IR poison semantics, FROUND round-half-away sign-of-zero, uitofp of values near 2^53/2^64. Confirm numerically (opt fold / x86).' },
  { key: 'X17-shift-bfe-value', loc: `${SRC}/NVPTXISelDAGToDAG.cpp tryBFE + ${SRC}/NVPTXInstrInfo.td shift/bfe/bfi/funnel patterns`,
    focus: 'A shift/bfe/bfi/funnel VALUE-MISCOMPILE beyond the found ones: bfe signedness/width edge (start+len>width), bfi insert mask, funnel shift amount mod, shift by exactly width, sra of i16/i8. Confirm a wrong value.' },
  { key: 'X18-vector-value', loc: `${SRC}/NVPTXISelLowering.cpp BUILD_VECTOR/EXTRACT/INSERT/SHUFFLE/lowerLoadVector/lowerSTOREVector + ${SRC}/NVPTXInstrInfo.td v4i8/v2i16/v2f16 patterns`,
    focus: 'A vector VALUE-MISCOMPILE: extractelement/insertelement of v4i8/v2i16 at a specific lane returning the wrong byte/half, shuffle mask, build_vector lane order, load/store vector element offset. Confirm via extractelement + llc that a lane is wrong.' },
]

phase('Sweep8')

const results = await parallel(TARGETS.map(t => () =>
  agent(`${BAR}\n\n=== YOUR AREA ===\nRead and analyze: ${t.loc}\n\nScrutinize: ${t.focus}\n\nRead ${README} first (exclusions), then the real source, confirm with llc, enumerate EVERY distinct instance. Return findings via structured output.`,
    { label: t.key, phase: 'Sweep8', schema: SCHEMA })
    .then(r => ({ key: t.key, findings: (r && r.findings) || [] }))
    .catch(() => ({ key: t.key, findings: [] }))
))

const all = []
for (const r of results) for (const f of (r.findings || [])) all.push({ region: r.key, ...f })
all.sort((a, b) => (b.confidence || 0) - (a.confidence || 0))
log(`Round8: ${all.length} raw findings across ${TARGETS.length} areas`)
return { count: all.length, findings: all }
