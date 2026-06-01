# 049 — bf16/tf32 mma.sync m16n8k8 under-guarded: emitted at sm_75/PTX 6.5 (requires sm_80/PTX 7.0)

- **Kind:** other (invalid PTX)
- **Reachable via:** llc -mcpu=sm_75
- **Component:** NVPTXIntrinsics.td 5112-5113 (clause), 5030-5163 (WMMA_REGINFO.Predicates cond), 5293-5300 & 5361-5387 (MMA uses FragA-only predicate)  (round-7 area `P04-wmma-mma-arch`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration (sibling guards/orderings); no local `ptxas` was available to execute the rejection.

## Summary

bf16/tf32 `mma.sync.m16n8k8` is under-guarded (FragA-only predicate) and emitted on sm_75/PTX6.x where it requires a newer target

## Mechanism / root cause

The plain MMA class derives its Requires from FragA only (MMA_OP_PREDICATES<FragA,b1op>, lines 5293-5300; class MMA at 5361, Requires<MMA_OP_PREDICATES<FragA,b1op>.ret> at 5368). FragA's arch guard is the !cond at lines 5030-5163. For an mma op with FragA element type bf16 or tf32 and geom m16n8k8, none of the type-specific clauses match (the bf16 clause 5093-5096 only covers m16n16k16/m8n32k16/m32n8k16; the tf32 clauses 5098-5102 only cover m16n16k8; the m16n8k4/m16n8k16/... mma clause 5125-5132 does not list m16n8k8). Execution therefore reaches the broad fall-through `!or(!eq(geom,"m16n8k8"), !eq(geom,"m8n8k16")) : [hasSM<75>, hasPTX<65>]` (lines 5112-5113), which was written for f16 m16n8k8 / int8 m8n8k16. So bf16 and tf32 m16n8k8 mma are guarded only by sm_75/PTX 6.5. Per the LLVM test generator's own ground truth (wmma.py: is_type_supported() returns ptx>=70 for bf16/tf32 at lines 519-520; bf16/tf32 mma require sm_80) and the PTX ISA (bf16/tf32 mma added in PTX ISA 7.0 / sm_80; the mnemonic mma.sync.aligned.m16n8k8.row.col.f32.bf16.bf16.f32 does not exist before PTX 7.0/sm_80), emitting these at .version 6.5 / .target sm_75 produces PTX that ptxas for that target/version rejects. Confirmed: wmma.py emits NO bf16/tf32 m16n8k8 at ptx65/sm75 but does at ptx70/sm80. Both the PTX version (6.5<7.0) and the sm (sm_75<sm_80) are under-guarded (the instruction is still emitted at sm_75/ptx70).

## Trigger

Call llvm.nvvm.mma.m16n8k8.row.col.bf16 (or .tf32) and compile with any -mcpu<sm_80 and/or -mattr=+ptx<70 (e.g. sm_75/ptx65). Backend silently emits mma.sync.aligned.m16n8k8.row.col.f32.{bf16|tf32}... under .target sm_75 / .version 6.5.

## Reproducer

```
declare {float,float,float,float} @llvm.nvvm.mma.m16n8k8.row.col.bf16(i32, i32, i32, float, float, float, float)
define {float,float,float,float} @test_bf16(i32 %a0, i32 %a1, i32 %b0, float %c0, float %c1, float %c2, float %c3) {
  %r = call {float,float,float,float} @llvm.nvvm.mma.m16n8k8.row.col.bf16(i32 %a0, i32 %a1, i32 %b0, float %c0, float %c1, float %c2, float %c3)
  ret {float,float,float,float} %r
}

; tf32 variant (a=4 regs, b=2 regs):
declare {float,float,float,float} @llvm.nvvm.mma.m16n8k8.row.col.tf32(i32, i32, i32, i32, i32, i32, float, float, float, float)
define {float,float,float,float} @test_tf32(i32 %a0, i32 %a1, i32 %a2, i32 %a3, i32 %b0, i32 %b1, float %c0, float %c1, float %c2, float %c3) {
  %r = call {float,float,float,float} @llvm.nvvm.mma.m16n8k8.row.col.tf32(i32 %a0, i32 %a1, i32 %a2, i32 %a3, i32 %b0, i32 %b1, float %c0, float %c1, float %c2, float %c3)
  ret {float,float,float,float} %r
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_75 -mattr=+ptx65 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.9).
