# w502 - VectorCombine `vectorizeLoadInsert` drops ALL load metadata when widening (`!invariant.load`, `!nontemporal`, `!range`, `!tbaa`, …)

## Location

`llvm/lib/Transforms/Vectorize/VectorCombine.cpp`

- Entry: `VectorCombine::vectorizeLoadInsert` line 237
- Defective code: line 347

```cpp
// line 344-350
IRBuilder<> Builder(Load);
Value *CastedPtr =
    Builder.CreatePointerBitCastOrAddrSpaceCast(SrcPtr, Builder.getPtrTy(AS));
Value *VecLd = Builder.CreateAlignedLoad(MinVecTy, CastedPtr, Alignment);
VecLd = Builder.CreateShuffleVector(VecLd, Mask);

replaceValue(I, *VecLd);
```

The widened vector load `VecLd` is created from scratch and is never
populated with any metadata from `Load`. There is no `copyMetadata`,
no `setAAMetadata`, no `setMetadata(LLVMContext::MD_invariant_load, ...)`,
etc. The original `Load`'s metadata is silently discarded.

This is the same defect class as the load-widening helpers in
`InstCombine` (which calls `setAAMetadata` and propagates
`MD_invariant_load`, `MD_nontemporal`, etc.).

Note: `widenSubvectorLoad` (line 358) and the scalarized loads emitted by
`scalarizeLoadExtract` (line 2130) have an analogous omission — but at
least `scalarizeLoadExtract` does call `NewLoad->setAAMetadata(...)`
on line 2139 (only for ConstantInt indices). `vectorizeLoadInsert`
propagates nothing.

## Repro (`repro.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define <4 x i32> @vli_inv(ptr dereferenceable(128) align 16 %p) {
  %x = load i32, ptr %p, align 16, !invariant.load !0
  %r = insertelement <4 x i32> poison, i32 %x, i32 0
  ret <4 x i32> %r
}
!0 = !{}
```

## Invocation

```
opt -mtriple=x86_64-unknown-linux-gnu -passes=vector-combine -S repro.ll
```

## Observed `opt` output

```llvm
define <4 x i32> @vli_inv(ptr align 16 dereferenceable(128) %p) {
  %1 = load <4 x i32>, ptr %p, align 16          ; <-- !invariant.load gone
  %r = shufflevector <4 x i32> %1, <4 x i32> poison, <4 x i32> <i32 0, i32 poison, i32 poison, i32 poison>
  ret <4 x i32> %r
}
```

Default x86 -O2 reproduces (`-O2 -S` of the same input):

```llvm
define <4 x i32> @vli_inv(ptr readonly align 16 captures(none) dereferenceable(128) %p) local_unnamed_addr #0 {
  %r = load <4 x i32>, ptr %p, align 16          ; <-- !invariant.load gone
  ret <4 x i32> %r
}
```

## Same path also drops `!nontemporal`, `!range`, `!tbaa`

For brevity, the same minimal pattern (insert at index 0 of a poison
vector, aligned dereferenceable pointer) reproduces with each of:

- `load i32 ..., !nontemporal !{i32 1}` → emitted vector load has no
  `!nontemporal` (and the backend therefore emits regular `movdqa`
  instead of `movntdqa`-style codegen, partially defeating the source
  programmer's intent).
- `load i32 ..., !range !{i32 0, i32 16}` → range information is lost.
  The narrow scalar value was constrained to [0,16); the widened vector
  is unconstrained.
- `load i32 ..., !tbaa !{...}` → all AA metadata is gone, so subsequent
  alias-analysis-driven optimizations (LICM, GVN, MemorySSA) see this
  load as accessing the conservative type universe and may pessimize
  scheduling / fail to disambiguate.

## Why each lost metadata matters

| Metadata | Lost-semantics consequence |
| --- | --- |
| `!invariant.load` | LICM / SLP-vectorize / GVN refuse to hoist or fuse loads that are no longer marked invariant. Downstream callers that *assumed* the load was invariant (e.g. constant lookup table reads) may be moved into hot loops. |
| `!nontemporal` | Backend loses the streaming-store/load hint. On x86 with AVX2/AVX-512 this means `vmovntdqa` is downgraded to `vmovdqa`, affecting cache behavior in tight kernels. |
| `!range` | KnownBits / value tracking weakens. InstCombine folds that depended on the bounded range (e.g. `cmp ult %v, 16` → `true`) no longer fire. |
| `!tbaa` / `!alias.scope` / `!noalias` | Subsequent AA queries return MayAlias more often. LICM may decline to hoist a store past this load; SROA / mem2reg-like cleanups may give up. |

## Fix sketch

Apply the same pattern used elsewhere in LLVM for load-widening:

```cpp
auto *NewLoadInst = cast<LoadInst>(VecLd);
// NB: must be done BEFORE the shuffle wrapping.
NewLoadInst->setMetadata(LLVMContext::MD_invariant_load,
                         Load->getMetadata(LLVMContext::MD_invariant_load));
NewLoadInst->setMetadata(LLVMContext::MD_nontemporal,
                         Load->getMetadata(LLVMContext::MD_nontemporal));
NewLoadInst->setAAMetadata(Load->getAAMetadata());
// !range is unsafe to copy verbatim to a wider scalar type, but for the
// element-0-only case we should at least drop it explicitly and consider
// constructing a range for the widened vector. Currently nothing is done.
```

For consistency, `widenSubvectorLoad` (line 358) needs the same
treatment.
