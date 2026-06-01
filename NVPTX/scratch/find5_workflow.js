export const meta = {
  name: 'nvptx-find-miscompiles-round5',
  description: 'Round 5: fresh angles + harder re-sweeps of productive veins (ABI/returns, scoped atomics, getTgtMem casts, sreg folding, feature predicates)',
  phases: [{ title: 'Sweep5' }],
}

const SRC = '/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX'
const INC = '/Users/justinlebar/code/vm-shared/llvm/llvm/include/llvm/IR'
const LLC = '/Users/justinlebar/code/llvm2/build/bin/llc'

const KNOWN = `
ALREADY-FOUND (do NOT re-report; find DIFFERENT bugs):
float->i1 setp.eq0; combineMulWide sext(shl nsw bits-1); PerformSELECTShiftCombine i64 & cross-width guard drop; int->bf16 double-round; byval param icmp/freeze/atomicrmw/cmpxchg ArgUseChecker (crash + cvta.param); va_arg i8 stride; overlapping aggregate load/store forward copy; ldu.global non-const align cast<ConstantInt>; AsmPrinter sub-byte vector global/splat/non-divisor + large-iN-global high-byte drop + ptrtoint-narrow-int-in-aggregate; NVVMIntrRange maxclusterrank/maxntid-0-dim/cluster_dim-0 APInt issues; tcgen05.ld/.st offset i32 trunc; tryLDG OOB on invariant ATOMIC_LOAD; replaceImageHandle select->SELP unreachable; internal ptx_kernel byval over-align; <N x i1> param OOB; scoped atomic min/max always signed; scoped f16/bf16 atom.add drops .noftz; kernel int param non-fundamental width (.u48); NVPTXLowerUnreachable isLoweredToTrap mismatch; LowerCall ArgOuts.size()==1 assert; InstCombine NVVM f16 min/max/fma FTZ fold.
NOT bugs (already refuted, skip): minimumnum/maximumnum signed-zero (PTX min/max DO order zeros); FTZ on cvt.f32.f16 (.ftz only affects f32 inputs); printHexu32imm -0x1U (valid PTX); inline-asm wrong-width constraint (user error); tryBFE GoodBits (missed-opt).
`

const BAR = `
You are hunting NEW CORRECTNESS BUGS in the LLVM NVPTX backend. Go DEEP.

WHAT COUNTS: (1) MISCOMPILE (emitted PTX/MIR != input IR for a well-defined non-UB input); (2) compiler SEGFAULT/OOB/UAF from valid input; (3) assertion from valid IR; (4) the backend emitting INVALID/unassemblable PTX for valid IR (e.g. a non-existent type/qualifier) — report these as kind 'other'.
WHAT DOESN'T: cannot-select/unsupported on genuinely unsupported ops; dropped metadata; missed-opt; perf; style; assert that degrades to graceful report_fatal_error in release; bugs only under UB/poison; spec-lawyering with no observable PTX difference. Verify "correct" PTX semantics against the actual PTX ISA, not assumptions.

${KNOWN}

RIGOR: specific defined input + exact wrong result/crash, tracing the code. USE the built llc at ${LLC} (set confirmed_with_llc); cross-check via llc -mtriple=x86_64 or opt folding. Empty array is fine.

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
  { key: 'V01-scoped-atom-resweep', loc: `${SRC}/NVPTXIntrinsics.td ATOM*/RED* multiclasses (2486-2760) + ${INC}/IntrinsicsNVVM.td atomic defs`,
    focus: 'The scoped-atomic family has already yielded 2 bugs (min/max always-signed; f16/bf16 add drops .noftz). Re-sweep ALL scoped atom/red variants for MORE: missing/wrong qualifier (.noftz, signedness, .sys/.gpu/.cta scope), an intrinsic with two patterns sharing one key (dead pattern -> wrong selection), atom.cas operand order, atom.exch, atom.and/or/xor on the wrong type, vector atom (f16x2/bf16x2) qualifiers, scoped vs non-scoped op divergence. List each emitted PTX op vs the intrinsic contract.' },
  { key: 'V02-gettgtmem-casts-hard', loc: `${SRC}/NVPTXISelLowering.cpp getTgtMemIntrinsic 4266-5602; cross-ref ImmArg in ${INC}/IntrinsicsNVVM.td`,
    focus: 'Grep EVERY cast<ConstantInt>/cast<ConstantSDNode>/cast<ConstantFPSDNode> and getConstantOperandVal in getTgtMemIntrinsic and the lowerIntrinsic* helpers. For each, find the intrinsic and check whether that operand index is marked ImmArg in IntrinsicsNVVM.td. Any unguarded checked-cast on a NON-ImmArg operand crashes on a runtime value (like ldu.global). Find intrinsics OTHER than ldu.global. Also flag any MOStore/MOLoad mismatch.' },
  { key: 'V03-struct-vector-return-abi', loc: `${SRC}/NVPTXISelLowering.cpp LowerReturn (search), return-value lowering, ${SRC}/NVPTXAsmPrinter.cpp retval .param decls`,
    focus: 'Return ABI: returning a struct by value, a vector (v2/v3/v4 of f16/i8/i32), an i1, a small int (signext/zeroext), multiple-field aggregate. Check the .param retval size/alignment/layout and field offsets match what the caller reads and the IR type. Find a return whose emitted layout or extension differs from IR (e.g. v3 vector padded wrong, i1 return, signext i8 return zero-extended).' },
  { key: 'V04-sreg-fold-bound', loc: `${SRC}/NVVMIntrRange.cpp (full), ${SRC}/NVVMReflect.cpp, and any constant-folding of ntid/ctaid/nctaid/cluster reads from reqntid/maxntid/cluster_dim/reqnctapercluster/maxnreg attributes`,
    focus: 'A correctness bug (not just the known APInt asserts): is any sreg read folded to a constant or given a !range using an UPPER-BOUND attribute (maxntid, maxclusterrank, maxnreg) as if it were EXACT (reqntid, cluster_dim, reqnctapercluster)? E.g. folding ntid.x to maxntid.x, or a range [1, maxntid+1) asserted as exact when the launch used fewer threads -> downstream code miscompiles. Trace which attribute each fold/range uses and whether it is exact.' },
  { key: 'V05-feature-predicate', loc: `${SRC}/NVPTXInstrInfo.td + ${SRC}/NVPTXIntrinsics.td Pat/Requires predicates (hasSM/hasPTX/hasArchAccelFeatures); ${SRC}/NVPTXSubtarget.h/.cpp`,
    focus: 'A pattern whose Requires<[...]> predicate is WRONG: gated on too-low an sm/ptx so it emits an instruction the target does not support (ptxas rejects), or two overlapping patterns where the wrong one wins on some subtarget, or a missing predicate letting a newer instruction be emitted for an old arch. Find a (sm,ptx) config where a valid IR op selects an instruction invalid for that arch, or selects a semantically-different fallback.' },
  { key: 'V06-atomicrmw-fp-expand', loc: `${SRC}/NVPTXISelLowering.cpp (atomicrmw fadd/fsub/fmax/fmin/fminimum/fmaximum handling, setOperationAction for ATOMIC_LOAD_F*, shouldExpandAtomicRMWInIR) + ${SRC}/NVPTXAtomicLower.cpp`,
    focus: 'Floating-point atomicrmw lowering: fadd to atom.add (FTZ?), fsub (negate then add - sign correct?), fmax/fmin to atom.max/min (signed-zero/NaN), fmin/fmax/fsub that must be expanded to a CAS loop - is the CAS loop correct (right comparison, right updated value, ABA)? Scoped FP atomics. Find an fp atomicrmw whose emitted code computes a different stored/returned value than the IR for a concrete input.' },
  { key: 'V07-vector-load-store-odd', loc: `${SRC}/NVPTXISelDAGToDAG.cpp tryLoadVector/tryStoreVector 1195-1502; ${SRC}/NVPTXISelLowering.cpp lowerLoadVector/lowerSTOREVector 3793-3968, ReplaceNodeResults vector cases`,
    focus: 'Vector loads/stores of odd shapes: v3 (3-element) vectors, v8/v16, sub-byte element vectors (v4i8), mixed alignment, <N x i1>. Element count -> ld.v2/ld.v4 grouping and the leftover element; offset of each chunk; element ORDER within ld.v4 (lane 0..3 mapping). Find a vector load/store where an element is loaded/stored at the wrong offset or wrong lane vs IR. Use extractelement/insertelement to confirm.' },
  { key: 'V08-fp-atomic-and-bitcast-vector', loc: `${SRC}/NVPTXISelLowering.cpp LowerBITCAST 2024-2052, LowerBUILD_VECTOR/INSERT/EXTRACT for v4i8/v2i16 (search getExtractVectorizedValue, PRMT-based pack/unpack)`,
    focus: 'v4i8 and v2i16 pack/unpack via PRMT/BFE: bitcast i32<->v4i8, build_vector of 4 i8, extract i8 lane k, insert i8 lane k. Check the PRMT selector / BFE offset for each lane index (0,1,2,3) is correct and the byte/halfword order matches little-endian IR semantics. Find a lane index that reads/writes the wrong byte.' },
  { key: 'V09-recent-features', loc: `${SRC}/NVPTXIntrinsics.td + ${SRC}/NVPTXISelLowering.cpp for recently-added: movmatrix, narrow-fp->bf16 conversions (cvt.rn.bf16x2 etc.), tcgen05.mma, clusterlaunchcontrol, st.bulk, cp.async.bulk.tensor`,
    focus: 'Recently added intrinsics/instructions: immediate-width truncation (like tcgen05 offset), operand ordering, wrong qualifier, fragment layout. Check movmatrix transpose direction, narrow-fp->bf16 conversion rounding/signedness, cp.async.bulk.tensor dims/im2col offsets/cache-hint/multicast operand placement. Find an operand built with too-narrow MVT or placed wrong.' },
  { key: 'V10-lowerargs-nonbyval', loc: `${SRC}/NVPTXLowerArgs.cpp (markPointerAsGlobal, handleByValParam non-byval pointer path, lowerArgs for grid_constant, generic-to-specific arg AS) + ${SRC}/NVPTXTagInvariantLoads.cpp`,
    focus: 'Beyond the known byval ArgUseChecker bug: handling of NON-byval pointer kernel args (markPointerAsGlobal assuming global AS when the pointer may be to another space), grid_constant non-byval, the address-space cast inserted for pointer args, and TagInvariantLoads tagging a load !invariant.load when the underlying memory could be written (the const __restrict__ assumption). Find a case where the AS assumption or invariance tag is unsound -> wrong load.' },
  { key: 'V11-asmprinter-const-more', loc: `${SRC}/NVPTXAsmPrinter.cpp constant emission (bufferLEByte ConstantExpr cases: add/sub/gep/inttoptr/bitcast; printWords/printBytes symbol+offset; getSymbolOffset; vectors of pointers; half/bfloat/fp128 constant bytes)`,
    focus: 'AsmPrinter constants have yielded 4 bugs. Find MORE: ConstantExpr add/sub/gep with a symbol (offset emitted correctly?), inttoptr in init, a vector of pointers, fp128/f80 byte emission, bfloat/half global byte order, an aggregate where padding bytes carry stale data, getSymbolOffset sign. Cross-check emitted bytes vs the IR constant (use x86 llc as reference).' },
  { key: 'V12-shift-funnel-more', loc: `${SRC}/NVPTXISelLowering.cpp lowerFSH/expandFSH64 3225-3274, lowerROT; ${SRC}/NVPTXInstrInfo.td funnel/rotate/shift patterns; FUN_SHFL_CLAMP/FUNSHFR`,
    focus: 'Funnel shift (fshl/fshr) and rotate beyond the known SELECT-clamp bug: expandFSH64 correctness (i64 funnel via shf), rotate by variable amount mod width, funnel shift where amount==0 or ==width, fshl/fshr with the two operands swapped, the shf.l/shf.r clamp vs wrap mode for the high/low result word. Find a concrete (x,y,amt) where the emitted funnel/rotate != IR.' },
]

phase('Sweep5')

const results = await parallel(TARGETS.map(t => () =>
  agent(`${BAR}\n\n=== YOUR AREA ===\nRead and analyze: ${t.loc}\n\nScrutinize: ${t.focus}\n\nRead the real source and confirm with llc. Return findings via structured output.`,
    { label: t.key, phase: 'Sweep5', schema: SCHEMA })
    .then(r => ({ key: t.key, findings: (r && r.findings) || [] }))
    .catch(() => ({ key: t.key, findings: [] }))
))

const all = []
for (const r of results) for (const f of (r.findings || [])) all.push({ region: r.key, ...f })
all.sort((a, b) => (b.confidence || 0) - (a.confidence || 0))
log(`Round5: ${all.length} raw findings across ${TARGETS.length} areas`)
return { count: all.length, findings: all }
