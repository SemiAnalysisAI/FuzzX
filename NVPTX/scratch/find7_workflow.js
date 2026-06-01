export const meta = {
  name: 'nvptx-find-round7',
  description: 'Round 7: exhaustive feature-predicate / address-space-validity / immediate-width / qualifier / crash sweep',
  phases: [{ title: 'Sweep7' }],
}

const SRC = '/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX'
const INC = '/Users/justinlebar/code/vm-shared/llvm/llvm/include/llvm/IR'
const LLC = '/Users/justinlebar/code/llvm2/build/bin/llc'
const README = '/Users/justinlebar/code/FuzzX/NVPTX/README.md'

const BAR = `
You are hunting NEW CORRECTNESS BUGS in the LLVM NVPTX backend. Be EXHAUSTIVE in your area and enumerate EVERY distinct instance you find (these classes often have many).

WHAT COUNTS: (1) MISCOMPILE (emitted PTX != input IR for a well-defined non-UB input); (2) compiler SEGFAULT/OOB/UAF/assert/stack-overflow from valid input; (3) backend emits INVALID/unassemblable PTX for valid IR — an instruction/qualifier/type/form the declared -mcpu/-mattr target does not support (ptxas rejects), or a malformed mnemonic — report as kind 'other'.
WHAT DOESN'T: cannot-select/unsupported on genuinely unsupported ops (a clean 'LLVM ERROR: Cannot select' is FINE, not a bug); dropped metadata; missed-opt; perf; style; report_fatal_error that is a graceful diagnostic; bugs only under UB/poison. Verify PTX semantics/version requirements against the actual PTX ISA.

EXCLUDE already-found bugs: FIRST read ${README} (40 confirmed bugs + a rejected list). Do NOT re-report those. In particular these instruction families ALREADY have a found bug — only report DIFFERENT instances/instructions: non-sync vote (arch guard), cvta.param (arch guard), add/sub ftz+sat order, sust.p subword, atom on const/param AS, scoped atom min/max signedness & cas guards & f16/bf16 .noftz, tcgen05 ld/st offset width, ldu.global align cast.

RIGOR: name the exact instruction emitted, the exact PTX ISA / sm version it requires (cite the rule), and show the emitted .target/.version is lower (or the form is malformed). USE ${LLC} to get the actual emitted PTX (set confirmed_with_llc). For crashes, give the exact valid IR and the abort/stack-trace. Cross-check "correct" via llc -mtriple=x86_64 or opt where relevant.

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
  // ---- feature-predicate / arch-validity, split by family (enumerate ALL under-guarded instances) ----
  { key: 'P01-atom-fp-arch', loc: `${SRC}/NVPTXIntrinsics.td atom/red FP families + Requires predicates; ${SRC}/NVPTXSubtarget.h has* helpers`,
    focus: 'atom/red FP variants emitted without (or with too-low) sm/ptx guard: atom.add.f64 (needs sm_60/ptx50), atom.add.f16/f16x2 (sm_70/ptx63), atom.add.bf16/bf16x2 (sm_90/ptx78), red.* FP, vector atom. For each, find one whose pattern lacks the guard and is emitted on a lower -mcpu. Enumerate every under-guarded one. (Skip the known scoped-add .noftz and min/max-signed bugs.)' },
  { key: 'P02-warp-arch', loc: `${SRC}/NVPTXIntrinsics.td warp ops (match.sync, redux.sync, activemask, bar.warp.sync, elect.sync, mapa, griddepcontrol) + predicates`,
    focus: 'Warp/CTA ops emitted without/too-low arch guard: match.sync (sm_70/ptx60), redux.sync (sm_80/ptx70), activemask (sm_62/ptx62?), elect.sync (sm_90/ptx80), bar.warp.sync, griddepcontrol (sm_90/ptx78). Find each whose pattern lacks the proper Requires and is emitted on a lower target. (Skip non-sync vote, already found.)' },
  { key: 'P03-cvt-narrowfp-arch', loc: `${SRC}/NVPTXIntrinsics.td + ${SRC}/NVPTXInstrInfo.td narrow-fp cvt (e4m3/e5m2 fp8, e2m1 fp4, e3m2/e2m3 fp6, tf32, bf16x2) + predicates`,
    focus: 'Narrow-fp conversion intrinsics emitted without arch guard: cvt to/from fp8 (sm_89/sm_90), fp6/fp4 (sm_100), tf32 (sm_80), bf16/bf16x2 cvt (sm_80/sm_90). Find each whose pattern is reachable on a lower -mcpu and emits a cvt the target lacks. Enumerate.' },
  { key: 'P04-wmma-mma-arch', loc: `${SRC}/NVPTXIntrinsics.td wmma/mma/ldmatrix/stmatrix/movmatrix + predicates`,
    focus: 'wmma/mma/ldmatrix variants emitted without/with-wrong arch guard so a fragment shape/type valid only on a newer arch is emitted on an older -mcpu (or the reverse: a guard so tight it never matches → skip, that is just cannot-select). Find genuinely under-guarded ones emitting target-invalid PTX.' },
  { key: 'P05-cpasync-mbarrier-arch', loc: `${SRC}/NVPTXIntrinsics.td cp.async*, mbarrier*, fence*, cluster*, st.bulk, discard, applypriority, prefetch + predicates`,
    focus: 'cp.async (sm_80/ptx70), cp.async.bulk (sm_90/ptx80), mbarrier (sm_80/ptx70), fence.proxy/mbarrier (sm_90), cluster.* (sm_90), prefetch/applypriority/discard. Find any emitted without proper arch guard on a lower -mcpu. Enumerate.' },
  { key: 'P06-f16-bf16-arith-arch', loc: `${SRC}/NVPTXInstrInfo.td f16/bf16 arithmetic & compare patterns + predicates; ${SRC}/NVPTXISelLowering.cpp set*Action for f16/bf16`,
    focus: 'f16 scalar/vector arith needs sm_53/ptx42; bf16 arith needs sm_80/ptx70; f16x2/bf16x2 fma/min/max/setp have specific arch reqs. Find an f16/bf16 op pattern emitted on a target that lacks it (e.g. add.bf16 on sm_70, or min.f16x2 on sm_53 where only later support exists), producing target-invalid PTX. Cross-check setOperationAction gating.' },
  // ---- address-space validity ----
  { key: 'A01-atom-red-as', loc: `${SRC}/NVPTXISelDAGToDAG.cpp getAddrSpace 499-513 + ${SRC}/MCTargetDesc/NVPTXInstPrinter.cpp printAtomicCode/addsp`,
    focus: 'Beyond const/param atom (found): does red.* (reduction) on const/param/local AS emit atom.const.red? Does shared::cluster atom emit on a non-cluster target? Does any atom/red path emit a .space qualifier ptxas rejects for that op? Enumerate the AS x op combinations that produce invalid PTX.' },
  { key: 'A02-ldst-cvta-special-as', loc: `${SRC}/NVPTXInstrInfo.td ld/st patterns + ${SRC}/NVPTXIntrinsics.td cvta/isspacep/mapa, prefetch, ld.global.nc; addrspace handling`,
    focus: 'ld/st/cvta/prefetch/ld.global.nc/isspacep on an address space the instruction does not support (e.g. ld.global.nc from non-global, prefetch on local, cvta on an AS without a cvta form, shared::cluster ld on non-cluster). Find AS combos that emit invalid PTX. Enumerate.' },
  // ---- immediate width ----
  { key: 'I01-dagtodag-imm', loc: `${SRC}/NVPTXISelDAGToDAG.cpp every getTargetConstant/getConstant building an instruction immediate (cp.async.bulk dims/masks, prefetch, fence scope, shfl control, tcgen05 non-offset operands, mbarrier count)`,
    focus: 'getTargetConstant(x.getZExtValue(), DL, MVT::iNN) where the source operand is WIDER than MVT::iNN (high bits dropped) — like the tcgen05 offset bug but DIFFERENT operands/routines. Check each Select* that materializes an immediate from a wider intrinsic operand. Enumerate.' },
  { key: 'I02-lowering-imm', loc: `${SRC}/NVPTXISelLowering.cpp lower* intrinsic handlers building immediates (lowerCvtRS, lowerPrmt, tcgen05/tensormap/clusterlaunchcontrol, getPRMT)`,
    focus: 'Hand-written intrinsic lowerings that build a target constant or selector from a wider operand with a narrow MVT, or compute a selector/mask incorrectly. Find a truncation or wrong constant. Enumerate.' },
  // ---- wrong qualifier ----
  { key: 'Q01-math-qualifier', loc: `${SRC}/NVPTXIntrinsics.td math op asm strings (rcp/sqrt/rsqrt/div/sin/cos/lg2/ex2/tanh and the F_MATH_* multiclasses)`,
    focus: 'Wrong/misordered/missing PTX qualifier in a math op asm string (like the add/sub .sat/.ftz transposition, but DIFFERENT ops): wrong rounding suffix, missing .ftz, .approx vs .rn, modifier order. Compare against sibling ops that are correct. Enumerate the malformed asm strings.' },
  { key: 'Q02-cvt-qualifier', loc: `${SRC}/NVPTXInstrInfo.td + ${SRC}/NVPTXIntrinsics.td cvt asm strings & modifier-printing (CvtMode, ftz/sat/rnd)`,
    focus: 'cvt patterns with wrong/misordered qualifiers: rounding mode wrong, .ftz/.sat in wrong order or wrong applicability, .relu, signedness of the cvt suffix. Find a cvt asm string ptxas would reject or that has wrong semantics. (Skip the known float->i1 and int->bf16 ones.) Enumerate.' },
  { key: 'Q03-atom-red-qualifier', loc: `${SRC}/NVPTXIntrinsics.td atom/red asm strings + ${SRC}/MCTargetDesc/NVPTXInstPrinter.cpp printAtomicCode (sem/scope/op suffixes)`,
    focus: 'atom/red qualifier ordering/correctness: sem (.relaxed/.acquire/.release/.acq_rel) + scope (.cta/.gpu/.sys/.cluster) + space + op ordering; a wrong or missing qualifier vs PTX grammar; the .noftz on more FP atom ops. (Skip the found f16/bf16 add .noftz.) Enumerate.' },
  // ---- crashes ----
  { key: 'C01-asmprinter-const-crash', loc: `${SRC}/NVPTXAsmPrinter.cpp bufferLEByte/bufferAggregateConstant/printScalarConstant for: vector-of-pointers, array of fp128, packed struct padding, named struct, ConstantExpr (gep/inttoptr/bitcast/select), float80/ppc_fp128, large vectors`,
    focus: 'More AsmPrinter constant-emission crashes (llvm_unreachable/assert) or wrong bytes for valid global initializers beyond the known sub-byte/large-int/ptrtoint/fp128 ones: vector of pointers, ppc_fp128/x86_fp80, ConstantExpr inttoptr/select, packed struct trailing padding, zero-size. Enumerate.' },
  { key: 'C02-selection-crash', loc: `${SRC}/NVPTXISelDAGToDAG.cpp + ${SRC}/NVPTXISelLowering.cpp for unchecked cast<>/getConstantOperandVal/switch-default-unreachable reachable from valid IR`,
    focus: 'More crashes like ldu-align-cast / tryLDG-OOB / image-handle-unreachable: an unguarded cast<ConstantInt>/cast<ConstantSDNode> on a non-ImmArg operand, a getConstantOperandVal past the operand list for some node kind, a switch default llvm_unreachable reachable from valid IR. Enumerate distinct ones.' },
  { key: 'C03-pass-crash', loc: `${SRC}/NVPTXLowerArgs.cpp, NVPTXAtomicLower.cpp, NVPTXGenericToNVVM.cpp, NVPTXLowerAlloca.cpp, NVPTXCtorDtorLowering.cpp, NVPTXProxyRegErasure.cpp, NVPTXImageOptimizer.cpp`,
    focus: 'Crashes in the NVPTX IR/MI passes on valid input: a visitor/switch default, an unchecked cast, an assumption about a global/instruction shape that valid IR can violate, iterator invalidation. (LowerArgs byval+icmp/atomicrmw already found — find DIFFERENT.) Enumerate.' },
  { key: 'C04-mc-dwarf-crash', loc: `${SRC}/MCTargetDesc/NVPTXInstPrinter.cpp, NVPTXMCExpr.cpp, ${SRC}/NVPTXDwarfDebug.cpp, ${SRC}/NVPTXAsmPrinter.cpp debug/symbol paths`,
    focus: 'Crashes in MC/printing/debug: InstPrinter modifier method on an out-of-range operand, MCExpr eval, debug-info emission null-deref/assert on valid IR with debug metadata, symbol emission. Enumerate.' },
  // ---- a few more miscompile-focused ----
  { key: 'M01-vector-lane-misc', loc: `${SRC}/NVPTXInstrInfo.td + ${SRC}/NVPTXISelLowering.cpp v2f16/v2bf16/v2i16/v4i8 binop & pack/unpack patterns; combinePackingMovIntoStore`,
    focus: 'A packed-vector op or pack/unpack that produces the WRONG VALUE (lanes swapped, wrong byte, wrong element) for a concrete defined input — a true value-miscompile, confirmed via extractelement + llc. Prioritize finding a real wrong-value bug here.' },
  { key: 'M02-conversion-value-misc', loc: `${SRC}/NVPTXISelLowering.cpp LowerINT_TO_FP/FP_TO_INT/FP_ROUND/FP_EXTEND/FROUND + ${SRC}/NVPTXInstrInfo.td cvt patterns`,
    focus: 'A conversion that produces the WRONG VALUE for a concrete defined input (wrong rounding/sign/saturation), beyond the known float->i1 and int->bf16: e.g. fptoui/fptosi saturation, f64->f16 double round, FROUND sign-of-zero, uitofp of large i64. Prioritize a true value-miscompile confirmed numerically.' },
  { key: 'M03-combine-value-misc', loc: `${SRC}/NVPTXISelLowering.cpp PerformADDCombine/PerformMULCombine/combineMADConstOne/combineMulSelectConstOne/PerformSETCCCombine/PerformVSELECTCombine/combinePRMT/PerformEXTRACTCombine`,
    focus: 'A DAG combine that changes the numeric result for a defined input (wrong identity, off-by-one constant, operand swap, predicate flip, lane mask). Prioritize a true value-miscompile confirmed via llc + x86/opt cross-check.' },
  { key: 'M04-atomicrmw-fp-misc', loc: `${SRC}/NVPTXISelLowering.cpp atomicrmw fsub/fmin/fmax/fminimum/fmaximum/nand expansion + ${SRC}/NVPTXAtomicLower.cpp + ${SRC}/NVPTXIntrinsics.td atom min/max f`,
    focus: 'An fp/int atomicrmw (other than the found fadd-f32-FTZ) whose emitted code computes a wrong stored/returned value: fsub sign, fmin/fmax signed-zero/NaN, fmax CAS-loop comparison, integer min/max width<32 sign, nand. Confirm with a concrete input.' },
  { key: 'M05-int-arith-misc', loc: `${SRC}/NVPTXInstrInfo.td + ${SRC}/NVPTXISelLowering.cpp integer mul.wide/mad/div/rem/shift/bfe/bfi/sad patterns`,
    focus: 'An integer op producing a wrong value: signed div-by-2^k missing rounding fixup, mul.wide/mad signedness, bfe/bfi width/offset off-by-one, sad accumulate, rem sign. (Skip the known combineMulWide one.) Confirm numerically.' },
  { key: 'M06-select-setcc-fp', loc: `${SRC}/NVPTXInstrInfo.td setp/selp patterns for fp ordered/unordered compares; ${SRC}/NVPTXISelLowering.cpp lowerSELECT/PerformSETCCCombine`,
    focus: 'An fp compare/select emitting the wrong setp suffix (ordered setp.lt vs unordered setp.ltu for the wrong ISD CC, NaN handling), or selp operand order, giving a wrong boolean for a NaN/sign-boundary input. Confirm with a concrete input (NaN).' },
]

phase('Sweep7')

const results = await parallel(TARGETS.map(t => () =>
  agent(`${BAR}\n\n=== YOUR AREA ===\nRead and analyze: ${t.loc}\n\nScrutinize: ${t.focus}\n\nRead ${README} first (exclusions), then the real source, and confirm with llc. Enumerate EVERY distinct instance. Return findings via structured output.`,
    { label: t.key, phase: 'Sweep7', schema: SCHEMA })
    .then(r => ({ key: t.key, findings: (r && r.findings) || [] }))
    .catch(() => ({ key: t.key, findings: [] }))
))

const all = []
for (const r of results) for (const f of (r.findings || [])) all.push({ region: r.key, ...f })
all.sort((a, b) => (b.confidence || 0) - (a.confidence || 0))
log(`Round7: ${all.length} raw findings across ${TARGETS.length} areas`)
return { count: all.length, findings: all }
