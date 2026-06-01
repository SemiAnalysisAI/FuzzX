# 050 — e4m3/e5m2 mma.sync m16n8k32 (f8f6f4 plain form) under-guarded: emitted at PTX 8.4 (requires PTX 8.7)

- **Kind:** other (invalid PTX)
- **Reachable via:** llc -mcpu=sm_89 -mattr=+ptx84
- **Component:** NVPTXIntrinsics.td 5059-5060 (clause), 5055-5057 (the ptx87 clause that only covers m16n8k16), 5293-5300 & 5361-5387 (MMA FragA-only predicate)  (round-7 area `P04-wmma-mma-arch`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration (sibling guards/orderings); no local `ptxas` was available to execute the rejection.

## Summary

e4m3/e5m2 `mma.sync.m16n8k32` (f8f6f4) is guarded only at PTX 8.4 but requires PTX 8.7 (the m16n8k16 sibling clause does not cover k32)

## Mechanism / root cause

Same FragA-only mechanism. For a plain (kind="") mma op with FragA element type e4m3/e5m2 and geom m16n8k32, the only e4m3/e5m2-specific arch clause that imposes PTX 8.7 is `!and(!or(e4m3,e5m2), !eq(geom,"m16n8k16")) : [hasSM<89>, hasPTX<87>]` (lines 5055-5057) -- it matches geom m16n8k16 only. m16n8k32 therefore falls through to the generic fp8 clause `!or(e4m3,e5m2) : [hasSM<89>, hasPTX<84>]` (lines 5059-5060), so the m16n8k32 f8f6f4 plain mma is guarded at only PTX 8.4. Per the LLVM test generator (wmma.py is_mma_variant_supported lines 591-599: m16n8k16 OR m16n8k32 with e4m3/e5m2 and ptx_version<87 => unsupported) and the PTX ISA 8.7 release notes (the m16n8k32 / f8f6f4 datapath with e4m3/e5m2/e3m2/e2m3/e2m1 was introduced in PTX ISA 8.7, CUDA 12.8), mma.sync.aligned.m16n8k32.row.col.f32.e4m3.e4m3.f32 (and .e5m2...) does not exist before PTX 8.7; emitting it under .version 8.4 produces PTX that ptxas-8.4 rejects. Verified by differential: wmma.py emits NO m16n8k32 fp8 at ptx84, but does at ptx87; the sibling m16n8k16 fp8 is correctly cannot-select at ptx84 (rescued by clause 5055-5057) while m16n8k32 is wrongly emitted. The sm_89 part of the guard is correct (wmma.py emits m16n8k32 fp8 at ptx87/sm89); only the PTX-version bound is wrong. Note: the sparse MMA_SP/MMA_SP_BLOCK_SCALE paths are NOT affected because they Requires the listconcat of all four frags' predicates (lines 5488-5491), so the f32 accumulator frag pulls in the higher guard.

## Trigger

Call llvm.nvvm.mma.m16n8k32.row.col.f32.e4m3.e4m3.f32 (or .e5m2.e5m2.) and compile with -mcpu=sm_89 -mattr=+ptx84 (any ptx in [84,86]). Backend emits the bare f8f6f4 mma under .version 8.4.

## Reproducer

```
declare {float,float,float,float} @llvm.nvvm.mma.m16n8k32.row.col.f32.e4m3.e4m3.f32(i32, i32, i32, i32, i32, i32, float, float, float, float)
define {float,float,float,float} @t(i32 %a0,i32 %a1,i32 %a2,i32 %a3,i32 %b0,i32 %b1, float %c0, float %c1, float %c2, float %c3) {
  %r = call {float,float,float,float} @llvm.nvvm.mma.m16n8k32.row.col.f32.e4m3.e4m3.f32(i32 %a0,i32 %a1,i32 %a2,i32 %a3,i32 %b0,i32 %b1, float %c0, float %c1, float %c2, float %c3)
  ret {float,float,float,float} %r
}
; e5m2 variant: replace e4m3 with e5m2 in the intrinsic name (same signature).
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_89 -mattr=+ptx84 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.85).
