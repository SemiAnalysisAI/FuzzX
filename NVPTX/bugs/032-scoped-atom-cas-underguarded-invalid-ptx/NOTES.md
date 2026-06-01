# 032 — Scoped 16-bit CAS intrinsic emits atom.cas.b16 with no SM/PTX guard (invalid PTX on sm_50/sm_60, pre-PTX 6.3)

- **Kind:** other (invalid PTX)
- **Reachable via:** default llc, sm_50/sm_60
- **Component:** NVPTXIntrinsics.td 2540-2578 (F_ATOMIC_3, no Requires), 2637 (INT_PTX_ATOM_CAS_16), 2679-2696 (F_ATOMIC_3_INTRINSIC_PATTERN, no predicate), 2736 (ATOM3_cas_impl _b16)  (round-5 area `V05-feature-predicate`)
- **Candidate id:** r5_05

## Summary

scoped `atom.cas` patterns carry no subtarget predicate: `atom.cta.cas.b16` emitted below sm_70/PTX6.3 and `.cta/.sys` scope below sm_60 (ptxas rejects)

## Mechanism / root cause

The 16-bit scoped CAS path has NO subtarget predicate anywhere. The instruction multiclass F_ATOMIC_3 (def _rr/_ir/_ri/_ii, lines 2544-2562) carries no Requires<>, and the pattern multiclass F_ATOMIC_3_INTRINSIC_PATTERN (lines 2679-2696) emits Pat<> entries with no predicate. ATOM3_cas_impl instantiates _b16 -> INT_PTX_ATOM_CAS_16 ("atom...cas.b16", line 2637/2736) for the int_nvvm_atomic_cas_gen_i_{cta,sys} intrinsics on any i16. The subtarget already has the correct gate `hasAtomCas16() { return SmVersion >= 70 && PTXVersion >= 63; }` (NVPTXSubtarget.h:105) but it is never referenced in the .td. Per the PTX ISA, .b16 atom.cas was introduced in PTX ISA 6.3 (the same release that added sm_75 / atom .f16 add). So on sm_50/sm_60 (whose minimum PTX is 4.0/5.0) the backend silently emits an instruction that does not exist for that arch; ptxas rejects `atom.cta.cas.b16` on sm_60. Note the generic IR cmpxchg i16 path is safe (getMinCmpXchgSizeInBits()==32 widens it to atom.cas.b32); only the explicit scoped CAS NVVM intrinsic with i16 reaches the unguarded instruction.

## Trigger

Call @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr, i16, i16) (or the .sys variant) and compile for any target below sm_70 / below PTX 6.3, e.g. -mcpu=sm_60 (default PTX 5.0) or -mcpu=sm_50 (PTX 4.0).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

declare i16 @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr, i16, i16)

define i16 @cas16(ptr %p, i16 %cmp, i16 %new) {
  %r = call i16 @llvm.nvvm.atomic.cas.gen.i.cta.i16.p0(ptr %p, i16 %cmp, i16 %new)
  ret i16 %r
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_60 -o - repro.ll`

## Observed (wrong) output

```
.version 5.0
.target sm_60
.address_size 64
...
	ld.param.b64 	%rd1, [cas16_param_0];
	ld.param.b16 	%rs1, [cas16_param_1];
	ld.param.b16 	%rs2, [cas16_param_2];
	atom.cta.cas.b16 	%rs3, [%rd1], %rs1, %rs2;
	cvt.u32.u16 	%r1, %rs3;
	st.param.b32 	[func_retval0], %r1;
	ret;

(exit 0, no diagnostic. With -mcpu=sm_50 the same atom.cta.cas.b16 is emitted under .version 4.0 / .target sm_50. NVPTXGenDAGISel.inc shows the INT_PTX_ATOM_CAS_16_rr selection has no OPC_CheckPatternPredicate; hasAtomCas16() in NVPTXSubtarget.h:105 is referenced 0 times in the backend. The generic cmpxchg i16 path, by contrast, widens to atom.sys.cas.b32 and is safe.)
```

## Expected

The backend must guard the 16-bit scoped CAS path on hasAtomCas16() (SmVersion>=70 && PTXVersion>=63), and the scoped CAS patterns generally on hasAtomScope() (SmVersion>=60). For -mcpu=sm_60 (PTX 5.0) or -mcpu=sm_50, selecting int_nvvm_atomic_cas_gen_i_{cta,sys}.i16 should NOT match this instruction; the compiler should either error (no legal lowering) or emit a target-valid sequence (e.g. a 32-bit-CAS emulation, like the generic cmpxchg i16 path that already widens to atom.sys.cas.b32). It must never silently emit `atom.cta.cas.b16` under `.target sm_60`/`.version 5.0` (or `.target sm_50`/`.version 4.0`), because ptxas rejects atom.*.cas.b16 below sm_70 / PTX 6.3 (and rejects the .cta/.sys scope qualifier at all below sm_60).

## Related (same root cause)

The scope guard is also missing (sibling finding): Scoped atom.cas intrinsics emit .cta/.sys scope qualifier with no sm_60 guard -> invalid PTX on pre-sm_60 (and atom.cas.b16 on pre-sm_70)

All scoped 2-operand atomics route through ATOM2S_impl (line 2673), which appends `hasAtomScope` (= SmVersion>=60, NVPTXSubtarget.h:102) to every pattern's predicate list. The scoped CAS path is different: ATOM3_cas_impl uses F_ATOMIC_3_INTRINSIC_PATTERN (line 2679), whose four `def : Pat<...>` (lines 2683-2693) carry NO Requires<> predicate, and the target instructions INT_PTX_ATOM_CAS_{16,32,64}_rr (built by F_ATOMIC_3 at line 2540, instantiated 2635-2639) have `Predicates = []` (verified in tblgen --print-records). These CAS instructions are shared with the regular cmpxchg path, where scope/sem are derived from the IR atomic node and the generic legalizer suppresses the scope qualifier on unsupported targets. But the scoped-intrinsic Pat HARDCODES the scope to Scope_cta/Scope_sys (PatLeaf i32 1/4) with no version gating, so the printer (NVPTXInstPrinter.cpp:328-339) unconditionally em

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 0.88).

> NOTE ON KIND: The true category is "other" (the backend emits PTX that is invalid for the declared target — ptxas rejects it — rather than computing a numerically wrong result). The schema enum has no "other" value, so I select "miscompile" as the closest (the backend produces incorrect/un-assemblable machine output for the target). It is NOT a classic value-miscompile, NOT a crash, and NOT not-a-bug.

CONFIRMED real backend bug: the NVPTX backend emits an instruction (atom.cta.cas.b16) with no SM/PTX subtarget guard for the scoped 16-bit CAS NVVM intrinsic.

Source confirmation (NVPTXIntrinsics.td):
- multiclass F_ATOMIC_3 (line 2540) takes NO preds parameter and attaches no Requires<>, unlike F_ATOMIC_2 (line 2514: `list<Predicate> preds = []`). So the INT_PTX_ATOM_CAS_16 instruction (lines 2635-2639, F_ATOMIC_3<I16RT,"cas.b16",...>) carries no predicate.
- multiclass F_ATOMIC_3_INTRINSIC_PATTERN (lines 2679-2696) emits Pat<> for int_nvvm_atomic_cas_gen_i_{cta,sys} with NO predicate — not even hasAtomScope, which the analogous 2-op scoped path ATOM2S_impl (line 2674) DOES add via !listconcat(Preds,[hasAtomScope]).
- ATOM3_cas_impl _b16 (line 2736) -> INT_PTX_ATOM_CAS_16.
- NVPTXS
