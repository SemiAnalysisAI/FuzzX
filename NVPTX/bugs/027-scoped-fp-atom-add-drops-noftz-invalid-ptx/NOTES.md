# 027 — Scoped f16/bf16 atomic add (llvm.nvvm.atomic.add.gen.f.{cta,sys}) drops mandatory .noftz qualifier, emitting malformed PTX with wrong FTZ semantics

- **Kind:** other (invalid PTX)
- **Reachable via:** default llc, sm_90+/f16 any
- **Component:** NVPTXIntrinsics.td 2703-2706, 2657, 2486  (round-4 area `U01-atom-intrinsics`)
- **Candidate id:** r4_02

## Summary

scoped f16/bf16 `llvm.nvvm.atomic.add.gen.f.{cta,sys}` emits `atom.cta.add.f16` without the mandatory `.noftz` (ptxas rejects; non-scoped path is correct)

## Mechanism / root cause

The scoped FP atomic-add tablegen path builds the PTX op string without `.noftz` for half/bfloat. In ATOM2_add_impl the f16/bf16 variants are:
  defm _bf16 : ATOM2S_impl<OpStr, "f", "bf16", BF16RT, [hasSM<90>, hasPTX<78>]>;
  defm _f16  : ATOM2S_impl<OpStr, "f", "f16", F16RT, []>;
These feed TypeStr="f16"/"bf16" into ATOM2N_impl, which sets op_str = OpStr # "." # TypeStr = "add.f16"/"add.bf16", and F_ATOMIC_2_INTRINSIC builds asm_str = "atom" # sem_str # as_str # "." # op_str -> e.g. "atom.cta.add.f16". There is NO .noftz inserted anywhere on this path.

Contrast the non-scoped path (lines 2586-2587) which hardcodes the qualifier:
  defm INT_PTX_ATOM_ADD_F16  : F_ATOMIC_2<F16RT, atomic_load_fadd, "add.noftz.f16", ...>;
  defm INT_PTX_ATOM_ADD_BF16 : F_ATOMIC_2<BF16RT, atomic_load_fadd, "add.noftz.bf16", ...>;
and correctly emits e.g. `atom.relaxed.sys.add.noftz.f16`.

The PTX ISA requires the .noftz qualifier for atom.add on .f16/.f16x2/.bf16/.bf16x2 (it is part of the opcode for these types; ptxas rejects atom.add.f16 without it). So the scoped output is malformed PTX. Even if a tool tolerated it, omitting .noftz selects flush-to-zero denormal handling, which differs from the noftz (denormals-preserved) semantics the IR intends and that the non-scoped path uses — i.e. the emitted op does not match the intrinsic. The f32/f64 scoped variants are unaffected (those types legitimately have no .noftz).

## Trigger

Call llvm.nvvm.atomic.add.gen.f.cta.f16 / .sys.f16 / .cta.bf16 / .sys.bf16 (the scoped half/bfloat atomicAdd_block / atomicAdd_system intrinsics). f16 has no SM/PTX predicate so it triggers even on sm_70/ptx63; bf16 requires sm_90/ptx78.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

define half @t_h_cta(ptr %p, half %v) {
  %r = call half @llvm.nvvm.atomic.add.gen.f.cta.f16.p0(ptr %p, half %v)
  ret half %r
}
define bfloat @t_bf_sys(ptr %p, bfloat %v) {
  %r = call bfloat @llvm.nvvm.atomic.add.gen.f.sys.bf16.p0(ptr %p, bfloat %v)
  ret bfloat %r
}
declare half @llvm.nvvm.atomic.add.gen.f.cta.f16.p0(ptr, half)
declare bfloat @llvm.nvvm.atomic.add.gen.f.sys.bf16.p0(ptr, bfloat)
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_90 -mattr=+ptx78 -o - repro.ll`

## Observed (wrong) output

```
atom.cta.add.f16 	%rs2, [%rd1], %rs1;
...
	atom.sys.add.bf16 	%rs2, [%rd1], %rs1;

(grep "noftz" of the full output: 0 matches. f16 variant also reproduces on -mcpu=sm_70 -mattr=+ptx63, emitting `atom.cta.add.f16`.)
```

## Expected

The PTX must include the mandatory .noftz qualifier for half/bfloat atomic add, matching the non-scoped path:
	atom.cta.add.noftz.f16 	%rs2, [%rd1], %rs1;
	atom.sys.add.noftz.bf16 	%rs2, [%rd1], %rs1;
(The non-scoped atomicrmw fadd path correctly emits e.g. `atom.acquire.sys.add.noftz.f16`. ptxas requires .noftz for .f16/.f16x2/.bf16/.bf16x2 atom.add; without it the emitted PTX is malformed and, if tolerated, has wrong flush-to-zero denormal semantics.)

## Verification

Verified empirically with the built llc (reproduced directly by the orchestrator). 

> CONFIRMED real bug. The scoped FP atomic-add tablegen path emits PTX without the mandatory .noftz qualifier for half/bfloat types.

Source mechanism (NVPTXIntrinsics.td) verified line-by-line:
- ATOM2_add_impl<"add"> is instantiated at line 2741 (OpStr="add").
- f16/bf16 go through ATOM2S_impl (lines 2703-2704) -> ATOM2N_impl, which builds op_str = OpStr # "." # TypeStr = "add.f16"/"add.bf16" (line 2657).
- F_ATOMIC_2_INTRINSIC builds asm_str = "atom" # sem_str # as_str # "." # op_str (line 2486). No .noftz is inserted on this path.
- Contrast: the non-scoped path hardcodes "add.noftz.f16"/"add.noftz.bf16" (lines 2586-2587).

Empirical result: llc emits `atom.cta.add.f16` and `atom.sys.add.bf16` — both missing .noftz. grep for "noftz" in the scoped output returns 0 matches. By comparison, non-scoped atomicrmw fadd on the same types correctly emits `atom.acquire.sys.add.noftz.f16` / `atom.acquire.sys.add.noftz.bf16`.

The PTX ISA grammar is `atom{.sem}{.scope}{.space}.add.noftz.f16` (and .f16x2/.bf16/.bf16x2): .noftz is a MANDATORY qualifier for these types, not optional. ptxas rejects `atom.add.f16` without it. So the scoped output is malformed PTX that will not assemble. The backe
