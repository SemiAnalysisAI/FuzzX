# w637: `LowerAtomic` drops `volatile`, alignment, and AA metadata when lowering atomicrmw / cmpxchg / fence to plain load/store

## Severity
- **Volatile drop**: Definite miscompile - `atomicrmw volatile` and
  `cmpxchg volatile` have specified side-effect semantics that the lowered IR
  silently violates.
- **Alignment upgrade**: UB / fault risk on misaligned data.
- **TBAA / alias.scope / noalias drop**: alias-analysis misinformation /
  miscompile risk.

## Source

`llvm/lib/Transforms/Utils/LowerAtomic.cpp:22-50` (cmpxchg helpers) and
`129-143` (atomicrmw helper).

```cpp
// lines 22-50
bool llvm::lowerAtomicCmpXchgInst(AtomicCmpXchgInst *CXI) {
  IRBuilder<> Builder(CXI);
  Value *Ptr = CXI->getPointerOperand();
  Value *Cmp = CXI->getCompareOperand();
  Value *Val = CXI->getNewValOperand();

  auto [Orig, Equal] =
      buildCmpXchgValue(Builder, Ptr, Cmp, Val, CXI->getAlign());
  ...
}

std::pair<Value *, Value *> llvm::buildCmpXchgValue(IRBuilderBase &Builder,
                                                    Value *Ptr, Value *Cmp,
                                                    Value *Val,
                                                    Align Alignment) {
  LoadInst *Orig = Builder.CreateAlignedLoad(Val->getType(), Ptr, Alignment);
  Value *Equal = Builder.CreateICmpEQ(Orig, Cmp);
  Value *Res = Builder.CreateSelect(Equal, Val, Orig);
  Builder.CreateAlignedStore(Res, Ptr, Alignment);     // <-- no volatile, no MD
  return {Orig, Equal};
}
```

```cpp
// lines 129-143
bool llvm::lowerAtomicRMWInst(AtomicRMWInst *RMWI) {
  IRBuilder<> Builder(RMWI);
  Builder.setIsFPConstrained(
      RMWI->getFunction()->hasFnAttribute(Attribute::StrictFP));

  Value *Ptr = RMWI->getPointerOperand();
  Value *Val = RMWI->getValOperand();

  LoadInst *Orig = Builder.CreateLoad(Val->getType(), Ptr);    // <-- no align
  Value *Res = buildAtomicRMWValue(RMWI->getOperation(), Builder, Orig, Val);
  Builder.CreateStore(Res, Ptr);                               // <-- no align
  RMWI->replaceAllUsesWith(Orig);
  RMWI->eraseFromParent();
  return true;
}
```

Three concrete defects in `lowerAtomicRMWInst`:
- `CreateLoad` / `CreateStore` with no explicit `Align` get ABI alignment
  inferred from the type, which **upgrades** the alignment if the original
  atomicrmw used `align 1` on an i32 (or similar). Codegen then assumes
  natural alignment and may emit faulting instructions on a target where the
  data actually is unaligned.
- `isVolatile()` is never propagated. The original `atomicrmw volatile` is
  required to behave like a volatile memory access; the produced plain
  load/store can be reordered or elided.
- AA metadata (`!tbaa`, `!alias.scope`, `!noalias`, `!noalias_addrspace`,
  `!access_group`) is never propagated. Subsequent passes can wrongly
  conclude the new load/store doesn't alias things it actually does.

`lowerAtomicCmpXchgInst` has the volatile and metadata defects but does
preserve alignment via `buildCmpXchgValue(..., CXI->getAlign())`.

`buildAtomicRMWValue` / `buildCmpXchgValue` are also exposed library
functions used elsewhere (e.g. by AMDGPU custom lowerings), so any caller
relying on the defaults inherits the same hazard.

## Repro 1 - `lowerAtomicRMWInst` drops `volatile` and `!tbaa`, upgrades alignment

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @rmw_volatile_align1(ptr %p, i32 %v) {
  %r = atomicrmw volatile add ptr %p, i32 %v seq_cst, align 1, !tbaa !0
  ret i32 %r
}

!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C++ TBAA"}
```

```console
$ opt -passes=lower-atomic -S repro.ll
define i32 @rmw_volatile_align1(ptr %p, i32 %v) {
  %1 = load i32, ptr %p, align 4          ; volatile dropped, align 1 -> 4, !tbaa lost
  %new = add i32 %1, %v
  store i32 %new, ptr %p, align 4         ; same losses
  ret i32 %1
}
```

Diff vs source IR:

- `atomicrmw volatile` -> non-volatile load/store
- `align 1` -> `align 4` (ABI alignment of i32 silently inserted by `IRBuilder::CreateLoad/CreateStore`)
- `!tbaa !0` -> removed

## Repro 2 - `lowerAtomicCmpXchgInst` drops `volatile` and `!tbaa`

```llvm
target triple = "x86_64-unknown-linux-gnu"

define { i32, i1 } @cas_volatile(ptr %p, i32 %c, i32 %n) {
  %r = cmpxchg volatile ptr %p, i32 %c, i32 %n seq_cst seq_cst, align 4, !tbaa !0
  ret { i32, i1 } %r
}

!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C++ TBAA"}
```

```console
$ opt -passes=lower-atomic -S repro.ll
define { i32, i1 } @cas_volatile(ptr %p, i32 %c, i32 %n) {
  %1 = load i32, ptr %p, align 4          ; volatile dropped, !tbaa lost
  %2 = icmp eq i32 %1, %c
  %3 = select i1 %2, i32 %n, i32 %1
  store i32 %3, ptr %p, align 4           ; volatile dropped, !tbaa lost
  ...
}
```

Note: `cmpxchg`'s store is unconditional here (via `select` between old and
new). That's the standard "lowered cmpxchg without atomicity" trick - but
dropping `volatile` plus the unconditional store means LLVM is now free to
remove or hoist that store. For an MMIO register simulated by the
"non-preemptible environment" this pass targets, that is a behavior change.

## Why the existing safety nets miss

`LowerAtomic`'s documented contract (file comment, lines 7-12) is "lowers
atomic intrinsics to non-atomic form for use in a known non-preemptible
environment". The intent is that the data still has to behave like memory,
just not atomic memory. None of the three properties dropped here are about
atomicity - they're about per-access semantics that must survive lowering.

Compare with `AtomicExpandPass.cpp`'s `convertAtomicXchgToIntegerType`
(lines 579-607), which propagates volatility and calls
`copyMetadataForAtomic(*NewRMWI, *RMWI)`. That same template should apply
here.

## Suggested fix

In `lowerAtomicRMWInst`:
1. Call `CreateAlignedLoad(Val->getType(), Ptr, RMWI->getAlign())` and
   `CreateAlignedStore(Res, Ptr, RMWI->getAlign())`.
2. Set `Orig->setVolatile(RMWI->isVolatile())` and same on the store.
3. Copy AA metadata (mirror `copyMetadataForAtomic` from `AtomicExpandPass`,
   or just `Orig->copyMetadata(*RMWI, {LLVMContext::MD_tbaa, ...})`).

Same for `lowerAtomicCmpXchgInst` / `buildCmpXchgValue` (propagate `volatile`
and AA metadata; alignment already plumbed).

## opt/llc diff

This pass is exposed as `opt -passes=lower-atomic` (registered in
`PassRegistry.def:485`). The repros above show the input/output IR diff. No
target-specific configuration is required - the bug is purely in the lowering
utility.
