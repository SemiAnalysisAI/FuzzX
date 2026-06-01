export const meta = {
  name: 'nvptx-find-miscompiles',
  description: 'Fan out code-reading agents across the NVPTX backend to find miscompiles/segfaults',
  phases: [{ title: 'Find' }],
}

const SRC = '/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX'

const BAR = `
You are hunting for CORRECTNESS BUGS in the LLVM NVPTX backend (the GPU/PTX code generator).

WHAT COUNTS (in priority order):
1. MISCOMPILES: the generated PTX/MIR computes a different result than the input LLVM IR semantics for some well-defined (non-UB) input. This is the top priority. Examples: wrong constant, swapped operands, wrong sign/zero extension, wrong shift amount/clamp, off-by-one width, dropped chain/side-effect, wrong predicate, wrong address space, miscomputed mask/selector, incorrect fold in a DAGCombine that changes the numeric result, atomic/volatile semantics changed, etc.
2. COMPILER SEGFAULTS / out-of-bounds / null-deref / use-after-free / iterator invalidation in the backend that a plausible (valid) input can trigger.
3. (Low value, but acceptable) assertion failures reachable from valid IR.

WHAT DOES NOT COUNT (do NOT report these):
- "Cannot select", "unsupported", fatal_error / report_fatal_error on genuinely unsupported operations.
- Dropped metadata, missed optimizations, suboptimal-but-correct code, performance, missing diagnostics.
- Pure code-style / NFC concerns.

RIGOR: For a miscompile claim you must identify a SPECIFIC defined input for which the output differs from IR semantics, and explain the exact numeric/semantic discrepancy. "This looks suspicious" is not enough — trace the logic. Prefer fewer, well-reasoned findings over many shallow guesses. If after careful reading you find nothing solid in your region, return an empty findings array — that is a perfectly good answer.

For each finding provide:
- title: one-line summary
- file, lines: exact location (e.g. NVPTXISelLowering.cpp, 2250-2268)
- kind: miscompile | segfault | assertion | other
- mechanism: precise explanation of WHY the output is wrong, tracing the buggy code path. Include the relevant code excerpt.
- trigger: the conditions to hit it (target sm_XX/ptx version, types, operand values).
- ir: a concrete LLVM IR snippet that should exercise the bug when run through llc with -mtriple=nvptx64 (include the function and any declares). Make it minimal and self-contained.
- llc_cmd: the llc flags you'd use (e.g. "-mtriple=nvptx64 -mcpu=sm_90 -O2").
- confidence: 0.0-1.0, your honest probability this is a real bug.

You may use the built compiler at /Users/justinlebar/code/llvm2/build/bin/llc to sanity-check IR if helpful, but it is optional. Do NOT spend long on it; reasoning is primary.
`

const SCHEMA = {
  type: 'object',
  additionalProperties: false,
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        properties: {
          title: { type: 'string' },
          file: { type: 'string' },
          lines: { type: 'string' },
          kind: { type: 'string', enum: ['miscompile', 'segfault', 'assertion', 'other'] },
          mechanism: { type: 'string' },
          trigger: { type: 'string' },
          ir: { type: 'string' },
          llc_cmd: { type: 'string' },
          confidence: { type: 'number' },
        },
        required: ['title', 'file', 'lines', 'kind', 'mechanism', 'trigger', 'ir', 'llc_cmd', 'confidence'],
      },
    },
  },
  required: ['findings'],
}

const TARGETS = [
  // ---- NVPTXISelLowering.cpp ----
  { key: 'L01-combines-mul-shl', file: 'NVPTXISelLowering.cpp', region: '6353-6660',
    focus: 'combineMulWide, TryMULWIDECombine (mul.wide.s/u — sign correctness, operand width, which half), matchMADConstOnePattern, combineMADConstOne, combineMulSelectConstOne, PerformMULCombine, PerformSHLCombine. Watch for signed vs unsigned widening, off-by-one bit widths, x*(C+1)=>x*C+x style rewrites with wrong operand.' },
  { key: 'L02-combines-fadd-fma-store', file: 'NVPTXISelLowering.cpp', region: '5697-6210',
    focus: 'allowFMA, performFADDCombineWithOperands, combinePackingMovIntoStore, combineSTORE, combineLOAD, PerformADDCombine, mayFoldFMULIntoFMA. Watch for FMA contraction without contract/fast flags changing results, packing wrong lanes into a store, combining loads/stores that drop volatile/ordering or reorder.' },
  { key: 'L03-combines-setcc-extract-vsel', file: 'NVPTXISelLowering.cpp', region: '6209-6360',
    focus: 'performScalarizeV2F32Op, PerformFMinMaxCombine, PerformREMCombine. Watch for NaN/signed-zero handling in min/max, srem/urem sign, scalarization recombining v2f32 lanes wrongly.' },
  { key: 'L03b-combines-setcc-extract', file: 'NVPTXISelLowering.cpp', region: '6628-6960',
    focus: 'PerformSETCCCombine, PerformEXTRACTCombine, PerformSELECTShiftCombine, PerformVSELECTCombine, combineADDRSPACECAST, combinePRMT. Watch for wrong setcc predicate after commute, extract from build_vector picking wrong element, select->shift folds with wrong amount, vselect lane mask errors, addrspacecast folding that changes address space, PRMT selector miscomputation.' },
  { key: 'L04-combines-proxy-f16-dispatch', file: 'NVPTXISelLowering.cpp', region: '6937-7438',
    focus: 'combinePRMT, sinkProxyReg, getF16SubOpc, combineF16AddWithNeg (a+(-b) => sub: sign/operand order), combineIntrinsicWOChain, combineProxyReg, PerformDAGCombine dispatch. Watch for fadd(x, fneg(y))=>fsub with operands swapped, proxyreg sinking past chain, dispatch routing a node to the wrong combine.' },
  { key: 'L05-replace-knownbits-demanded', file: 'NVPTXISelLowering.cpp', region: '7438-7863',
    focus: 'ReplaceNodeResults (legalization of results — wrong extension/truncation/bitcast), getPreferredFPToIntOpcode, computeKnownBitsForTargetNode (over-claiming known bits => downstream miscompile), canonicalizePRMTInput, simplifyDemandedBitsForPRMT, SimplifyDemandedBitsForTargetNode (dropping demanded bits that matter).' },
  { key: 'L06-shift-funnel-rot-frem', file: 'NVPTXISelLowering.cpp', region: '2215-3308',
    focus: 'LowerShiftRightParts, LowerShiftLeftParts, lowerCTLZCTPOP, expandFSH64, lowerFSH (funnel shift — shift amount modulo width, clamp vs wrap), lowerROT (rotate amount), lowerFREM. Watch for shift-amount masking (mod bitwidth) errors, clamp vs modulo for shf, rotate by 0/width, frem sign.' },
  { key: 'L07-fp-lowering', file: 'NVPTXISelLowering.cpp', region: '2333-2574',
    focus: 'LowerFCOPYSIGN, LowerFROUND/FROUND32/FROUND64 (round-half-away-from-zero correctness, sign of zero, large magnitudes), PromoteBinOpToF32/PromoteBinOpIfF32FTZ (promoting f16/bf16 op to f32 then rounding — double rounding / FTZ), LowerINT_TO_FP, LowerFP_TO_INT, LowerFP_ROUND, LowerFP_EXTEND, LowerVectorArith.' },
  { key: 'L08-vector-lowering', file: 'NVPTXISelLowering.cpp', region: '347-2215',
    focus: 'getExtractVectorizedValue, buildTreeReduction (reassociation of non-reassociable reductions), LowerVECREDUCE, LowerBITCAST, LowerBUILD_VECTOR, LowerEXTRACT_VECTOR_ELT, LowerINSERT_VECTOR_ELT, LowerVECTOR_SHUFFLE (shuffle mask indexing, undef/poison lanes). Watch for wrong element index, lane packing for v2{f16,bf16,i16}, reduction order changing result for fadd/fmul without reassoc.' },
  { key: 'L09-loadstore-lowering', file: 'NVPTXISelLowering.cpp', region: '3308-4051',
    focus: 'lowerSELECT, lowerMSTORE (masked store — which lanes), LowerLOAD/LowerLOADi1, LowerMLOAD (masked load passthrough), lowerSTOREVector/LowerSTORE/LowerSTOREi1, LowerCopyToReg_128, lowerLoadVector. Watch for i1 load/store extension, masked load/store lane mask, vector element order, dropping volatile/ordering, 128-bit split.' },
  { key: 'L10-prmt-bswap-cvt-tcgen', file: 'NVPTXISelLowering.cpp', region: '1891-2950',
    focus: 'getPRMT, lowerBSWAP (byte permute selector for 16/32/64-bit), lowerCvtRSIntrinsics, lowerPrmtIntrinsic, tcgen05/tensormap intrinsic lowering, lowerIntrinsicVoid. Watch for wrong PRMT selector constant (byte indices), bswap selector for each width, mode constants.' },
  { key: 'L11-call-args', file: 'NVPTXISelLowering.cpp', region: '1365-1783',
    focus: 'correctParamType (bitcast/extension to expected param VT), LowerCall (argument marshalling, byval handling, alignment, vararg, return value extraction). Watch for wrong extension (sign vs zero) of small args/returns, wrong byte offsets, dropped args.' },
  { key: 'L11b-formal-args', file: 'NVPTXISelLowering.cpp', region: '4037-4253',
    focus: 'splitValueIntoRegisterParts, getParamSymbol, getCallParamSymbol, LowerFormalArguments. Watch for wrong extension of incoming small-int params, wrong part assembly, alignment.' },
  { key: 'L12-misc-stack-vaarg-addrspace', file: 'NVPTXISelLowering.cpp', region: '1783-1891',
    focus: 'LowerDYNAMIC_STACKALLOC, LowerSTACKRESTORE, LowerSTACKSAVE — alignment, address space of stack pointer.' },
  { key: 'L12b-addrspace-vaarg', file: 'NVPTXISelLowering.cpp', region: '3542-3793',
    focus: 'LowerADDRSPACECAST (null handling per AS, generic<->specific), LowerVAARG (alignment, type size, increment), LowerVASTART. Watch for vaarg overflow alignment and size rounding, addrspacecast of null.' },
  { key: 'M-getTgtMemIntrinsic', file: 'NVPTXISelLowering.cpp', region: '4266-5602',
    focus: 'getTgtMemIntrinsic: giant switch assigning MachineMemOperand info (MemVT size, MOLoad/MOStore/MOVolatile, align) for target intrinsics. Look for entries with the WRONG size/type (smaller than actual access => later code assumes wrong width), missing MOStore on a store intrinsic (or MOLoad on a load), or flags that let illegal reordering. Scan for anomalies vs sibling entries.' },

  // ---- NVPTXISelDAGToDAG.cpp ----
  { key: 'D1-load-store-sel', file: 'NVPTXISelDAGToDAG.cpp', region: '1105-1502',
    focus: 'SelectADDR, tryLoad, tryLoadVector, tryStore, tryStoreVector. Watch for wrong fromType/extension (sign vs zero) chosen for extending loads, wrong vector element ordering, address-space/ordering/volatile flags lost, alignment-based opcode selection that picks an unaligned op as aligned.' },
  { key: 'D2-ldg-ldu-bfe', file: 'NVPTXISelDAGToDAG.cpp', region: '1266-1714',
    focus: 'tryLDG, tryLDU (non-coherent loads — only valid for invariant/readonly?), tryBFE (bit field extract: start/len computation, sign vs zero extend, shift+and to bfe). Watch for BFE width/offset off-by-one, signed vs unsigned BFE, LDG used where memory may be mutated.' },
  { key: 'D3-addrspace-extract-unpack', file: 'NVPTXISelDAGToDAG.cpp', region: '413-1105',
    focus: 'SelectSETP_F16X2/BF16X2, tryUNPACK_VECTOR, tryEXTRACT_VECTOR_ELEMENT, SelectAddrSpaceCast. Watch for extracting wrong half of a packed vector, setp predicate per-lane, addrspacecast lowering choosing wrong cvta direction (to/from generic) or wrong address space number.' },
  { key: 'D4-i128-atomic-brjt-fence', file: 'NVPTXISelDAGToDAG.cpp', region: '1714-2360',
    focus: 'tryBF16ArithToFMA, SelectV2I64toI128, SelectI128toV2I64 (hi/lo ordering), selectAtomicSwap128, selectBR_JT (jump table index bounds/scaling), tryFence (ordering/scope mapping). Watch for hi/lo swapped in 128-bit splits, wrong fence ordering/scope, jump-table miscalculation.' },
  { key: 'D5-select-dispatch', file: 'NVPTXISelDAGToDAG.cpp', region: '99-413',
    focus: 'Select() dispatch and tryIntrinsicChain, SelectTcgen05Ld. Watch for a node routed to the wrong custom selector, opcode confusion.' },

  // ---- NVPTX IR passes (target codegenprepare-like; run for NVPTX, not CPUs) ----
  { key: 'P1-lower-args', file: 'NVPTXLowerArgs.cpp', region: 'all',
    focus: 'Lowering kernel/device function arguments, byval copies to local/param space, address-space inference, replacing pointer args. Watch for assuming an argument is in a particular address space when it may not be, copying wrong size for byval, mishandling aliasing, replacing uses incorrectly.' },
  { key: 'P2-atomic-aggrcopy-alloca', files: 'NVPTXAtomicLower.cpp NVPTXLowerAggrCopies.cpp NVPTXAllocaHoisting.cpp NVPTXLowerAlloca.cpp',
    focus: 'NVPTXAtomicLower (scoped atomic expansion correctness), NVPTXLowerAggrCopies (memcpy/memmove/memset -> loops: byte count, overlap for memmove, alignment, volatile, address spaces), AllocaHoisting, LowerAlloca (address space of alloca / cast). Watch for memmove lowered as forward copy (overlap bug), wrong element size in copy loop, dropping volatile/atomic.' },
  { key: 'P3-generic2nvvm-taginv-proxy-peephole', files: 'NVPTXGenericToNVVM.cpp NVPTXTagInvariantLoads.cpp NVPTXProxyRegErasure.cpp NVPTXPeephole.cpp NVPTXImageOptimizer.cpp',
    focus: 'GenericToNVVM (global var address-space rewriting), TagInvariantLoads (tagging loads as invariant when they may not be — correctness!), ProxyRegErasure (erasing proxy regs without preserving values), Peephole (machine peepholes that change semantics, e.g. addrspacecast+load fusion), ImageOptimizer. Watch especially TagInvariantLoads marking a mutable load invariant.' },
  { key: 'P4-nvvmreflect-intrrange-props-fwdparams', files: 'NVVMReflect.cpp NVVMIntrRange.cpp NVVMProperties.cpp NVPTXForwardParams.cpp NVPTXLowerUnreachable.cpp',
    focus: 'NVVMReflect (folding __nvvm_reflect to a constant — wrong constant?), NVVMIntrRange (adding !range metadata to sreg intrinsics like tid/ntid — if range is wrong/too tight it is a miscompile enabler), NVPTXForwardParams (forwarding param-space pointers — aliasing/lifetime), LowerUnreachable. Watch for IntrRange asserting a range that excludes legal values.' },
  { key: 'P5-tti-instcombine', file: 'NVPTXTargetTransformInfo.cpp', region: 'all',
    focus: 'instCombineIntrinsic / simplifyDemandedVectorElts / foldings for NVPTX intrinsics, isSourceOfDivergence, and any fold that rewrites an intrinsic to a constant or simpler form. Watch for a fold that produces a different value (e.g. wrong identity, wrong constant, ignoring rounding mode / ftz).' },
  { key: 'P6-instrinfo-branch', file: 'NVPTXInstrInfo.cpp', region: 'all',
    focus: 'analyzeBranch, insertBranch, removeBranch, reverseBranchCondition, isLoadFromStackSlot/StoreToStackSlot, copyPhysReg, isUnpredicatedTerminator. Watch for reverseBranchCondition inverting the wrong way, analyzeBranch misreporting fallthrough/targets => wrong CFG, copyPhysReg emitting a wrong-width move.' },

  // ---- Asm/constant emission ----
  { key: 'A1-asmprinter-constants', file: 'NVPTXAsmPrinter.cpp', region: 'all',
    focus: 'Constant emission: bufferAggregateConstant, bufferLEByte, printScalarConstant, emitGlobals, vector/sub-byte constant packing, float constant hex emission. A wrong byte/endianness/padding in an emitted constant is a miscompile. Watch for sub-byte packing order, struct padding, big/little-endian byte order, float bit emission.' },

  // ---- TableGen patterns ----
  { key: 'T1-instrinfo-td', file: 'NVPTXInstrInfo.td', region: 'all',
    focus: 'Patterns mapping ISD nodes to PTX instrs: shifts, rotates, conversions (cvt with rounding/saturation), bfe/bfi, selp/setp, min/max, mul.wide, sad, etc. Watch for a pattern that maps an ISD op to a PTX instr with different semantics (e.g. wrong rounding mode, signed where unsigned needed, missing .ftz, .sat applied incorrectly, predicate inversion).' },
  { key: 'T2-intrinsics-td', file: 'NVPTXIntrinsics.td', region: '1-3300',
    focus: 'Patterns/lowering for ld/st/atom/red/shfl/vote/redux and math intrinsics (first half of file). Watch for atom op mapped to wrong operation, shfl mode/mask, redux op, ld.global.nc used where not safe, wrong memory ordering qualifier on atomics.' },
  { key: 'T2b-intrinsics-td', file: 'NVPTXIntrinsics.td', region: '3300-6567',
    focus: 'Patterns/lowering for math/conversion/wmma/tcgen/cp.async and remaining intrinsics (second half). Watch for conversion intrinsics mapped to wrong cvt qualifiers (rounding/ftz/sat), wmma fragment layout, fp conversion sign.' },
]

phase('Find')

function promptFor(t) {
  const loc = t.region && t.region !== 'all'
    ? `Focus on this region of ${SRC}/${t.file}: lines ${t.region} (read it in full, plus enough surrounding context to understand it).`
    : t.file
      ? `Focus on the file ${SRC}/${t.file} (read it in full).`
      : `Focus on these files in ${SRC}: ${t.files} (read each in full).`
  return `${BAR}\n\n=== YOUR ASSIGNED REGION ===\n${loc}\n\nSpecific things to scrutinize: ${t.focus}\n\nRead the actual source with the Read/Grep tools before reasoning. Return your findings via the structured output.`
}

const results = await parallel(TARGETS.map(t => () =>
  agent(promptFor(t), { label: t.key, phase: 'Find', schema: SCHEMA })
    .then(r => ({ key: t.key, findings: (r && r.findings) || [] }))
    .catch(() => ({ key: t.key, findings: [] }))
))

// Flatten and tag with source region
const all = []
for (const r of results) for (const f of (r.findings || [])) all.push({ region: r.key, ...f })
all.sort((a, b) => (b.confidence || 0) - (a.confidence || 0))

log(`Collected ${all.length} raw findings across ${TARGETS.length} regions`)
return { count: all.length, findings: all }
