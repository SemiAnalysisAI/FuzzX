export const meta = {
  name: 'nvptx-verify-miscompiles',
  description: 'Empirically test + adversarially verify NVPTX miscompile candidates with the built llc',
  phases: [{ title: 'Verify' }, { title: 'Refute' }],
}

const LLC = '/Users/justinlebar/code/llvm2/build/bin/llc'
const SRC = '/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX'
const SCRATCH = '/Users/justinlebar/code/FuzzX/NVPTX/scratch'

// args is an array of candidate objects: {id, region, title, file, lines, kind, mechanism, trigger, ir, llc_cmd, confidence}
const CANDIDATES = [{"region": "V04-sreg-fold-bound", "title": "NVVMIntrRange: maxntid dimension product truncated from uint64_t to unsigned, producing too-tight (or empty) tid/ntid ranges -> miscompile and assertion", "file": "llvm/lib/Target/NVPTX/NVVMIntrRange.cpp", "lines": "79-93 (root cause at 79-80; consumed at 91-93 and via addRangeAttr 122-136)", "kind": "miscompile", "mechanism": "getOverallMaxNTID(F) returns std::optional<uint64_t> = the PRODUCT of the maxntid dimensions (NVVMProperties.cpp:264 -> getVectorProduct accumulates into uint64_t). NVVMIntrRange.cpp:79-80 truncates this to `unsigned`:\n\n  const unsigned MaxNTID = OverallMaxNTID.value_or(std::numeric_limits<unsigned>::max());\n\nA maxntid product can legitimately exceed UINT_MAX (the per-dimension values are arbitrary unsigned ints, e.g. maxntid=\"641,6700417\" with product 641*6700417 = 4294967297 = 2^32+1, a valid upper bound). After truncation MaxNTID becomes 1, so line 91-93 computes MaxBlockDim = {min(1024,1), min(1024,1), min(64,1)} = {1,1,1}. Then addRangeAttr gives tid.x range [0,1) (folded to constant 0) and ntid.x range [1,2) (folded to constant 1). The intended clamp `std::min(1024u, MaxNTID)` is defeated because the overflow happens BEFORE the min. Two manifestations of the same root cause: (a) product mod 2^32 in [1,1023] -> silently too-tight ranges -> MISCOMPILE; (b) product an exact multiple of 2^32 (e.g. maxntid=\"65536,65536,1\", product=2^32 -> truncates to 0) -> ntid.x gets range [1,1) which is Lower==Upper, neither min nor max -> ASSERTION in ConstantRange::ConstantRange (ConstantRange.cpp:58) via addRangeAttr (line 56). Fix: keep MaxNTID as uint64_t (or clamp to 1024 before narrowing). Note this is distinct from the already-known literal maxntid-0-dim case: there the user writes 0 and tid.x gets benign [0,0); here a large legitimate value silently overflows.", "trigger": "ptx_kernel function with \"nvvm.maxntid\" whose dimension product exceeds UINT_MAX. maxntid=\"641,6700417\" (product 2^32+1) -> tid.x folded to 0, ntid.x folded to 1; full -O2 collapses tid.x+ntid.x+tid.y+ntid.y to `ret i32 2` even though a valid launch (e.g. blockDim 256x256x1, product 65536 << 2^32+1) makes the true value up to 1022. maxntid=\"65536,65536,1\" (product 2^32) -> assertion crash on ntid.x.", "ir": "define ptx_kernel i32 @overflow_to_one() \"nvvm.maxntid\"=\"641,6700417\" {\n  %1 = call i32 @llvm.nvvm.read.ptx.sreg.tid.x()\n  %2 = call i32 @llvm.nvvm.read.ptx.sreg.ntid.x()\n  %3 = call i32 @llvm.nvvm.read.ptx.sreg.tid.y()\n  %4 = call i32 @llvm.nvvm.read.ptx.sreg.ntid.y()\n  %5 = add i32 %1, %2\n  %6 = add i32 %5, %3\n  %7 = add i32 %6, %4\n  ret i32 %7\n}\ndeclare i32 @llvm.nvvm.read.ptx.sreg.tid.x()\ndeclare i32 @llvm.nvvm.read.ptx.sreg.ntid.x()\ndeclare i32 @llvm.nvvm.read.ptx.sreg.tid.y()\ndeclare i32 @llvm.nvvm.read.ptx.sreg.ntid.y()\n\n; Assertion variant: change attr to \"nvvm.maxntid\"=\"65536,65536,1\" and use ntid.x.", "llc_cmd": "Miscompile (folds whole fn to `ret i32 2`): /Users/justinlebar/code/llvm2/build/bin/opt < t.ll -S -mtriple=nvptx64-nvidia-cuda -O2 ; range attach visible with -passes=nvvm-intr-range (tid.x -> range(i32 0,1), ntid.x -> range(i32 1,2)). Assertion variant (maxntid=\"65536,65536,1\"): /Users/justinlebar/code/llvm2/build/bin/opt < t.ll -S -mtriple=nvptx64-nvidia-cuda -passes=nvvm-intr-range  (Assertion failed: Lower == Upper, but they aren't min or max value!, ConstantRange.cpp:58).", "confidence": 0.9, "confirmed_with_llc": true, "id": "r5_01"}, {"region": "V11-asmprinter-const-more", "title": "bufferLEByte crashes (llvm_unreachable) on integer aggregate element that is symbol+offset (add/sub of ptrtoint)", "file": "/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX/NVPTXAsmPrinter.cpp", "lines": "1680-1693", "kind": "assertion", "mechanism": "In bufferLEByte's IntegerTyID case, a ConstantExpr element is handled only if (a) ConstantFoldConstant folds it to a ConstantInt, or (b) its top-level opcode is exactly Instruction::PtrToInt (lines 1686-1691). A symbol-relative integer whose offset is applied OUTSIDE the ptrtoint -- e.g. `add (i64 ptrtoint(@g), i64 16)` or `sub (i64 ptrtoint(@g), i64 8)` -- has top-level opcode Add/Sub, contains a symbol so cannot be folded to a ConstantInt, and is NOT PtrToInt. It falls through to `llvm_unreachable(\"unsupported integer const type\")` at line 1693. The same constant works (i) at top-level scalar globals (different path: prints `x = g+16`) and (ii) when the offset is inside the ptrtoint as a GEP (`ptrtoint(gep(@g,+2))` prints `{g+8}`), proving the backend intends to support symbol+offset integers; only the add/sub-outside-ptrtoint form in an aggregate is missed. x86 reference emits `g+16` / `g-8` correctly.", "trigger": "A global array/struct whose integer-typed element is `add(ptrtoint(@sym), C)` or `sub(ptrtoint(@sym), C)`. Reproduced: `@arr = addrspace(1) global [2 x i64] [i64 ptrtoint(ptr addrspace(1) @g to i64), i64 add(i64 ptrtoint(ptr addrspace(1) @g to i64), i64 16)]` crashes; the sub form crashes too.", "ir": "target triple = \"nvptx64-nvidia-cuda\"\n@g = addrspace(1) global i32 42\n@arr = addrspace(1) global [2 x i64] [i64 ptrtoint (ptr addrspace(1) @g to i64), i64 add (i64 ptrtoint (ptr addrspace(1) @g to i64), i64 16)]", "llc_cmd": "/Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64-nvidia-cuda -mcpu=sm_80 arr_ptrtoint.ll -o /dev/null   # -> \"unsupported integer const type\" UNREACHABLE at NVPTXAsmPrinter.cpp:1693. x86 ref emits {g, g+16}.", "confidence": 0.9, "confirmed_with_llc": true, "id": "r5_02"}, {"region": "V11-asmprinter-const-more", "title": "bufferLEByte crashes (llvm_unreachable \"unsupported type\") on fp128 element inside an array or struct", "file": "/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX/NVPTXAsmPrinter.cpp", "lines": "1696-1701,1728-1729", "kind": "assertion", "mechanism": "bufferLEByte's floating-point switch handles only HalfTyID/BFloatTyID/FloatTyID/DoubleTyID (lines 1696-1701); FP128TyID is absent, so an fp128 Constant reaches `default: llvm_unreachable(\"unsupported type\")` at line 1729. A bare top-level fp128 global works because printModuleLevelGV routes FP128TyID straight to bufferAggregateConstant, which has a dedicated FP128 branch (lines 1759-1765) that emits the 16 bytes. But an fp128 nested in a ConstantArray/ConstantStruct is emitted element-by-element via bufferLEByte (bufferAggregateConstant lines 1768-1799), so each fp128 element hits the unreachable. The bare-fp128 support proves fp128 globals are intended to be supported; the nested case is an omission, not genuine non-support. x86 reference emits the fp128 array/struct bytes correctly.", "trigger": "A global array or struct containing an fp128 element/field, e.g. `[2 x fp128]` or `{ i32, fp128 }`. Both reproduced as crashes.", "ir": "target triple = \"nvptx64-nvidia-cuda\"\n@arr = addrspace(1) global [2 x fp128] [fp128 0xL00000000000000003FFF000000000000, fp128 0xL00000000000000004000000000000000]", "llc_cmd": "/Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64-nvidia-cuda -mcpu=sm_80 fp128_arr.ll -o /dev/null   # -> \"unsupported type\" UNREACHABLE at NVPTXAsmPrinter.cpp:1729. Bare `@f = global fp128 ...` works (emits 16 bytes); only nested fp128 crashes. x86 ref emits the array bytes.", "confidence": 0.9, "confirmed_with_llc": true, "id": "r5_03"}, {"region": "V10-lowerargs-nonbyval", "title": "NVPTXTagInvariantLoads tags volatile loads as !invariant.load, dropping volatile semantics (lowered to ld.global.nc)", "file": "llvm/lib/Target/NVPTX/NVPTXTagInvariantLoads.cpp", "lines": "33-58, 60-81", "kind": "miscompile", "mechanism": "isInvariantLoad() never checks LI->isVolatile(). A `load volatile` from a `noalias readonly` kernel pointer arg in global AS therefore passes the predicate (lines 50-57) and gets stamped !invariant.load (markLoadsAsInvariant, lines 60-63). In ISel, canLowerToLDG (NVPTXISelDAGToDAG.cpp:785-786) returns true purely on N.isInvariant() (no isVolatile check), so the volatile load is lowered via tryLDG to `ld.global.nc` (the non-coherent read-only data cache). This silently drops volatile semantics: the access becomes cacheable, freely reorderable/CSE-able, and reads through a cache that PTX explicitly says is only valid for data not written during the kernel. Correct lowering (no invariant tag) is `ld.volatile.global.b32`. A volatile poll of memory written by another agent (e.g. a spin flag) would read a stale cached value. Confirmed by diffing: with noalias readonly -> `ld.global.nc.b32`; with a plain (non-readonly) arg the same volatile load -> `ld.volatile.global.b32`.", "trigger": "ptx_kernel with `noalias readonly` pointer arg; `load volatile i32` from it in addrspace(1); compile for sm_80. Output uses ld.global.nc (volatile dropped) instead of ld.volatile.global.", "ir": "target triple = \"nvptx64-unknown-cuda\"\ndefine ptx_kernel void @volload(ptr noalias readonly %a, ptr %out) {\n  %ag = addrspacecast ptr %a to ptr addrspace(1)\n  %v = load volatile i32, ptr addrspace(1) %ag, align 4\n  store i32 %v, ptr %out, align 4\n  ret void\n}", "llc_cmd": "/Users/justinlebar/code/llvm2/build/bin/llc -mcpu=sm_80 -o - /tmp/vol.ll  (emits ld.global.nc.b32; compare /tmp/vol2.ll without readonly -> ld.volatile.global.b32)", "confidence": 0.85, "confirmed_with_llc": true, "id": "r5_04"}, {"region": "V05-feature-predicate", "title": "Scoped 16-bit CAS intrinsic emits atom.cas.b16 with no SM/PTX guard (invalid PTX on sm_50/sm_60, pre-PTX 6.3)", "file": "/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX/NVPTXIntrinsics.td", "lines": "2540-2578 (F_ATOMIC_3, no Requires), 2637 (INT_PTX_ATOM_CAS_16), 2679-2696 (F_ATOMIC_3_INTRINSIC_PATTERN, no predicate), 2736 (ATOM3_cas_impl _b16)", "kind": "other", "mechanism": "The 16-bit scoped CAS path has NO subtarget predicate anywhere. The instruction multiclass F_ATOMIC_3 (def _rr/_ir/_ri/_ii, lines 2544-2562) carries no Requires<>, and the pattern multiclass F_ATOMIC_3_INTRINSIC_PATTERN (lines 2679-2696) emits Pat<> entries with no predicate. ATOM3_cas_impl instantiates _b16 -> INT_PTX_ATOM_CAS_16 (\"atom...cas.b16\", line 2637/2736) for the int_nvvm_atomic_cas_gen_i_{cta,sys} intrinsics on any i16. The subtarget already has the correct gate `hasAtomCas16() { return SmVersion >= 70 && PTXVersion >= 63; }` (NVPTXSubtarget.h:105) but it is never referenced in the .td. Per the PTX ISA, .b16 atom.cas was introduced in PTX ISA 6.3 (the same release that added sm_75 / atom .f16 add). So on sm_50/sm_60 (whose minimum PTX is 4.0/5.0) the backend silently emits an instruction that does not exist for that arch; ptxas rejects `atom.cta.cas.b16` on sm_60. Note the generic IR cmpxchg i16 path is safe (getMinCmpXchgSizeInBits()==32 widens it to atom.cas.b32); only the explicit scoped CAS NVVM intrinsic with i16 reaches the unguarded instruction.", "trigger": "Call @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr, i16, i16) (or the .sys variant) and compile for any target below sm_70 / below PTX 6.3, e.g. -mcpu=sm_60 (default PTX 5.0) or -mcpu=sm_50 (PTX 4.0).", "ir": "target triple = \"nvptx64-nvidia-cuda\"\n\ndeclare i16 @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr, i16, i16)\n\ndefine i16 @cas16(ptr %p, i16 %cmp, i16 %new) {\n  %r = call i16 @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr %p, i16 %cmp, i16 %new)\n  ret i16 %r\n}", "llc_cmd": "/Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64-nvidia-cuda -mcpu=sm_60 /tmp/cas16.ll -o -   (emits 'atom.cta.cas.b16 ...' under .version 5.0 / .target sm_60, exit 0, no diagnostic; same with -mcpu=sm_50 -> .version 4.0)", "confidence": 0.82, "confirmed_with_llc": true, "id": "r5_05"}, {"region": "V01-scoped-atom-resweep", "title": "Scoped atom.cas intrinsics emit .cta/.sys scope qualifier with no sm_60 guard -> invalid PTX on pre-sm_60 (and atom.cas.b16 on pre-sm_70)", "file": "/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX/NVPTXIntrinsics.td", "lines": "2679-2696 (F_ATOMIC_3_INTRINSIC_PATTERN), instances at 2734-2743; cf. 2666-2677 ATOM2S_impl which adds [hasAtomScope]", "kind": "other", "mechanism": "All scoped 2-operand atomics route through ATOM2S_impl (line 2673), which appends `hasAtomScope` (= SmVersion>=60, NVPTXSubtarget.h:102) to every pattern's predicate list. The scoped CAS path is different: ATOM3_cas_impl uses F_ATOMIC_3_INTRINSIC_PATTERN (line 2679), whose four `def : Pat<...>` (lines 2683-2693) carry NO Requires<> predicate, and the target instructions INT_PTX_ATOM_CAS_{16,32,64}_rr (built by F_ATOMIC_3 at line 2540, instantiated 2635-2639) have `Predicates = []` (verified in tblgen --print-records). These CAS instructions are shared with the regular cmpxchg path, where scope/sem are derived from the IR atomic node and the generic legalizer suppresses the scope qualifier on unsupported targets. But the scoped-intrinsic Pat HARDCODES the scope to Scope_cta/Scope_sys (PatLeaf i32 1/4) with no version gating, so the printer (NVPTXInstPrinter.cpp:328-339) unconditionally emits `.cta`/`.sys`. The `.cta`/`.sys`/`.gpu` scope qualifiers on `atom` were introduced in PTX ISA 6.0 / sm_60; emitting them under `.target sm_50` / `.version 4.0` is invalid PTX that ptxas rejects. LLVM itself proves it knows this: a regular cmpxchg with syncscope(\"block\") on sm_50 emits `atom.cas.b32` + `membar.cta` (scope dropped), but on sm_60 emits `atom.cta.cas.b32`. Additionally the b16 variant emits `atom.cta.cas.b16` even on sm_50/sm_60, but atom.cas.b16 requires sm_70/PTX6.3 (the reason the regular path was changed to avoid b16 CAS in PR#119349) -- so b16 is invalid below sm_70 regardless of the scope issue. The asymmetry is stark: sibling scoped `add` on sm_50 cleanly errors `Cannot select`, while scoped `cas` silently emits invalid PTX.", "trigger": "Call any int_nvvm_atomic_cas_gen_i_{cta,sys} (i16/i32/i64) and compile with -mcpu=sm_50 (or any sm<60): emits atom.cta.cas.bN / atom.sys.cas.bN under .target sm_50 / .version 4.0, which ptxas rejects. For i16, even sm_60 produces atom.cta.cas.b16 (needs sm_70).", "ir": "declare i32 @llvm.nvvm.atomic.cas.gen.i.cta.i32.p0(ptr, i32, i32)\ndefine i32 @scoped_cas_i32(ptr %p, i32 %c, i32 %s) {\n  %r = call i32 @llvm.nvvm.atomic.cas.gen.i.cta.i32.p0(ptr %p, i32 %c, i32 %s)\n  ret i32 %r\n}", "llc_cmd": "/Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64 -mcpu=sm_50 /tmp/final.ll -o -   # emits: .version 4.0 / .target sm_50 / atom.cta.cas.b32 %r3, [%rd1], %r1, %r2;  (invalid: .cta scope needs sm_60/PTX6.0). Contrast: scoped add on sm_50 -> 'LLVM ERROR: Cannot select'.", "confidence": 0.7, "confirmed_with_llc": true, "id": "r5_06"}, {"region": "V06-atomicrmw-fp-expand", "title": "atomicrmw fadd float lowers to atom.add.f32, which unconditionally flushes subnormals (FTZ) even in IEEE denormal mode", "file": "/Users/justinlebar/code/vm-shared/llvm/llvm/lib/Target/NVPTX/NVPTXISelLowering.cpp", "lines": "7478-7479 (shouldExpandAtomicRMWInIR), pattern at NVPTXIntrinsics.td:2588", "kind": "miscompile", "mechanism": "shouldExpandAtomicRMWInIR returns AtomicExpansionKind::None unconditionally for f32 FAdd:\n\n  if (Ty->isFloatTy())\n    return AtomicExpansionKind::None;\n\nThis selects the native PTX instruction (NVPTXIntrinsics.td:2588: F_ATOMIC_2<F32RT, atomic_load_fadd, \"add.f32\", ...>), i.e. `atom.<sem>.<scope>.add.f32`. Per the PTX ISA, `atom.add.f32` ALWAYS flushes subnormal inputs AND subnormal results to sign-preserving zero (it has no .ftz/.noftz qualifier; the flush is hardwired). The note for the f16/bf16 variants is `add.noftz.f16`/`add.noftz.bf16` precisely because those CAN avoid flushing, but f32 cannot.\n\nThe IR `atomicrmw fadd float` is a full IEEE floating-point add. In the function's default denormal-fp-math mode (\"ieee\"), subnormals must be preserved. The decision at line 7478 never consults the denormal mode, so even a strict-IEEE function gets the flushing instruction. This is inconsistent with how NVPTX lowers a plain `fadd float`, which emits non-flushing `add.rn.f32` (confirmed below), and inconsistent with x86 which expands atomicrmw fadd to a CAS loop with a real (non-flushing) `addss`. LLVM treats f32-add subnormal flushing as a miscompile in default mode (cf. llvm-project issue #161342, tagged backend:NVPTX/clang:codegen, with the analogous denormal-result-flushed-to-+0 example). f16/bf16 (.noftz) and f64 (no PTX flush) are NOT affected; only f32.", "trigger": "atomicrmw fadd ptr %p, float %v in a function with the default (ieee) f32 denormal mode. Concrete input: *%p = 0x00000001 (smallest positive subnormal, ~1.4e-45) and %v = 0x00000001. IEEE result: store 0x00000002 (~2.8e-45, still subnormal) and return old value 0x00000001. Emitted atom.add.f32 result: both subnormal inputs flush to +0.0, 0.0+0.0=+0.0, so it stores 0x00000000 and returns the old loaded value as +0.0 (0x00000000). Both the stored memory and the returned value differ from IR semantics. (Also triggers when only the RESULT is subnormal, e.g. largest-subnormal + tiny.)", "ir": "target triple = \"nvptx64-nvidia-cuda\"\ndefine float @fadd_f32_ieee(ptr %p, float %v) {\n  %r = atomicrmw fadd ptr %p, float %v monotonic\n  ret float %r\n}", "llc_cmd": "/Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64-nvidia-cuda -mcpu=sm_90 -mattr=+ptx78 /tmp/ftz_bug.ll -o -   # emits: atom.relaxed.sys.add.f32 %r2, [%rd1], %r1   (compare: plain `fadd float` emits non-flushing add.rn.f32; x86 -mtriple=x86_64 expands to addss+cmpxchg)", "confidence": 0.6, "confirmed_with_llc": true, "id": "r5_07"}];

const VERIFY_SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: {
    id: { type: 'string' },
    real: { type: 'boolean' },
    kind: { type: 'string', enum: ['miscompile', 'segfault', 'assertion', 'not-a-bug'] },
    confidence: { type: 'number' },
    reasoning: { type: 'string' },
    repro_ir: { type: 'string' },
    llc_cmd: { type: 'string' },
    actual_output: { type: 'string' },
    expected: { type: 'string' },
  },
  required: ['id', 'real', 'kind', 'confidence', 'reasoning', 'repro_ir', 'llc_cmd', 'actual_output', 'expected'],
}

const REFUTE_SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: {
    id: { type: 'string' },
    verdict: { type: 'string', enum: ['confirmed', 'refuted', 'uncertain'] },
    confidence: { type: 'number' },
    reasoning: { type: 'string' },
  },
  required: ['id', 'verdict', 'confidence', 'reasoning'],
}

function verifyPrompt(c) {
  return `You are empirically verifying a SUSPECTED bug in the LLVM NVPTX backend. Be rigorous and skeptical: most candidates turn out NOT to be bugs.

Candidate id: ${c.id}
Title: ${c.title}
Location: ${c.file} ${c.lines || ''}
Kind claimed: ${c.kind}
Mechanism claimed:
${c.mechanism}
Trigger: ${c.trigger}
Proposed IR:
${c.ir}
Proposed llc cmd: ${c.llc_cmd}

TOOLS:
- Built compiler with NVPTX enabled: ${LLC}
- NVPTX backend source: ${SRC}
- Scratch dir for temp files: ${SCRATCH} (write .ll files here, name them ${c.id}-*.ll)

WHAT TO DO:
1. Read the cited source lines to confirm the mechanism is real (not a misreading).
2. Write a minimal, VALID, non-UB LLVM IR reproducer to ${SCRATCH}/${c.id}.ll. Use 'poison' not 'undef' unless undef is the point. Make sure it is something the backend actually accepts (no "cannot select"); if it errors for an unrelated reason, adjust until the relevant path is exercised.
3. Run: ${LLC} <flags> ${SCRATCH}/${c.id}.ll -o - and inspect the PTX.
   - For a MISCOMPILE: show that the emitted PTX computes a different value than the input IR semantics for some concrete, well-defined input. Pin down the exact discrepancy (which instruction, what it computes vs what it should). If you cannot exhibit a concrete defined input where results differ, it is NOT a miscompile.
   - For a SEGFAULT/ASSERTION: show llc actually crashes/asserts on valid IR (capture the message and the stack). A crash only on invalid IR does NOT count.
4. Be adversarial against your own claim. Common reasons a candidate is NOT a bug: the IR-level semantics already permit the behavior (UB, poison, freeze); the value is masked/normalized elsewhere; the suspicious code is dead or guarded; the PTX is actually correct on closer reading; the input needed is UB.

STANDARDS:
- "miscompile" only if you have a concrete defined input with a demonstrable wrong result (or a clearly wrong emitted instruction whose semantics you can pin down).
- "segfault"/"assertion" only if you reproduced the crash with the built llc on valid IR (paste the real output).
- Otherwise "not-a-bug".

Fill the structured output. Put the EXACT minimal IR in repro_ir, the exact llc command (with the path) in llc_cmd, the relevant snippet of real llc output in actual_output, and what a correct compiler should produce in expected. Set real=true only for miscompile/segfault/assertion that you actually substantiated.`
}

function refutePrompt(v, c) {
  return `An independent verifier claims the following is a REAL bug in the LLVM NVPTX backend. Your job is to REFUTE it. Default to skepticism: if the evidence is not airtight, lean toward 'refuted' or 'uncertain'.

Candidate id: ${v.id}
Title: ${c ? c.title : ''}
Claimed kind: ${v.kind}
Verifier reasoning: ${v.reasoning}
Repro IR:
${v.repro_ir}
llc cmd: ${v.llc_cmd}
Claimed actual output: ${v.actual_output}
Claimed expected: ${v.expected}

TOOLS: built llc at ${LLC}; source at ${SRC}; scratch ${SCRATCH}.

DO:
1. Re-run the exact command yourself (write the IR to ${SCRATCH}/${v.id}-refute.ll). Confirm the output matches what the verifier claimed.
2. Independently check whether the "wrong" output is actually wrong:
   - Recompute the IR semantics by hand for a concrete input.
   - Recompute what the emitted PTX does for that same input (mind PTX instruction semantics: e.g. shf.l/r.clamp clamps shift>=32 to 32; shl/shr by >=width is UB-ish in LLVM but defined-zero in PTX for some ops; cvt rounding; mul.wide sign; selp; setp predicate).
   - If they agree, it is REFUTED.
3. Check the IR is valid and non-UB. If the bug only manifests under UB/poison, REFUTE.
4. For crashes: confirm the crash reproduces and that the input IR is valid (verifier-clean). If it only crashes on invalid IR, REFUTE.

Output verdict: 'confirmed' (you reproduced it AND independently agree it is a real wrong-result/crash on valid IR), 'refuted' (it is not a bug, or only under UB, or does not reproduce), or 'uncertain'.`
}

phase('Verify')

const withIds = CANDIDATES.map((c, i) => ({ ...c, id: c.id || `c${String(i + 1).padStart(3, '0')}` }))

const results = await pipeline(
  withIds,
  c => agent(verifyPrompt(c), { label: `verify:${c.id}`, phase: 'Verify', schema: VERIFY_SCHEMA })
        .then(v => ({ ...v, _cand: c }))
        .catch(e => ({ id: c.id, real: false, kind: 'not-a-bug', confidence: 0, reasoning: 'verify error: ' + String(e), repro_ir: '', llc_cmd: '', actual_output: '', expected: '', _cand: c })),
  (v) => {
    if (!v || !v.real) return { ...(v || {}), refute: { verdict: 'refuted', confidence: 1, reasoning: 'verifier judged not-a-bug' } }
    return agent(refutePrompt(v, v._cand), { label: `refute:${v.id}`, phase: 'Refute', schema: REFUTE_SCHEMA })
      .then(r => ({ ...v, refute: r }))
      .catch(e => ({ ...v, refute: { verdict: 'uncertain', confidence: 0, reasoning: 'refute error: ' + String(e) } }))
  }
)

const clean = results.filter(Boolean).map(v => {
  const { _cand, ...rest } = v
  return { ...rest, region: _cand && _cand.region, title: _cand && _cand.title, file: _cand && _cand.file, lines: _cand && _cand.lines }
})
const confirmed = clean.filter(v => v.real && v.refute && v.refute.verdict === 'confirmed')
const uncertain = clean.filter(v => v.real && v.refute && v.refute.verdict === 'uncertain')
log(`Verify done: ${confirmed.length} confirmed, ${uncertain.length} uncertain, ${clean.length - confirmed.length - uncertain.length} refuted/not-a-bug`)
return { confirmed, uncertain, all: clean }
