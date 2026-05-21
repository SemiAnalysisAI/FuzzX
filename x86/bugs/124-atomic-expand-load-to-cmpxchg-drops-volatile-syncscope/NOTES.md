# w66: AtomicExpandPass expandAtomicLoadToCmpXchg drops volatile and syncscope

## Root cause
`AtomicExpandPass::expandAtomicLoadToCmpXchg` rewrites an atomic load into an
AtomicCmpXchg loop seed but never preserves `LI->isVolatile()` and never passes
`LI->getSyncScopeID()` to the new `AtomicCmpXchg`.

```
llvm/lib/CodeGen/AtomicExpandPass.cpp:668
bool AtomicExpandImpl::expandAtomicLoadToCmpXchg(LoadInst *LI) {
  ReplacementIRBuilder Builder(LI, *DL);
  AtomicOrdering Order = LI->getOrdering();
  if (Order == AtomicOrdering::Unordered)
    Order = AtomicOrdering::Monotonic;

  Value *Addr = LI->getPointerOperand();
  Type *Ty = LI->getType();
  Constant *DummyVal = Constant::getNullValue(Ty);

  Value *Pair = Builder.CreateAtomicCmpXchg(
      Addr, DummyVal, DummyVal, LI->getAlign(), Order,
      AtomicCmpXchgInst::getStrongestFailureOrdering(Order));
  // BUG: no Pair->setVolatile(LI->isVolatile());
  // BUG: no SSID passed to CreateAtomicCmpXchg; defaults to System.
  ...
}
```

Compare to the sibling helpers `convertAtomicLoadToIntegerType` (line 556) and
`convertAtomicStoreToIntegerType` (line 697), both of which carefully call
`setVolatile`, `setAtomic`, and propagate SyncScopeID.

## Trigger condition (x86)
`X86TargetLowering::shouldExpandAtomicLoadInIR` returns `CmpXChg` for i128
atomic loads when `cx16` is available. Reproducer:

```
target triple = "x86_64-unknown-linux-gnu"

define i128 @f1(ptr %p) {
  %v = load atomic volatile i128, ptr %p syncscope("singlethread") seq_cst, align 16
  ret i128 %v
}

define i128 @f2(ptr %p) {
  %v = load atomic i128, ptr %p syncscope("singlethread") seq_cst, align 16
  ret i128 %v
}

define i128 @f3(ptr %p) {
  %v = load atomic volatile i128, ptr %p seq_cst, align 16
  ret i128 %v
}
```

After `llc -mtriple=x86_64-unknown-linux-gnu -mattr=+cx16 -stop-after=atomic-expand`:

```
define i128 @f1(ptr %p) #0 {
  %1 = cmpxchg ptr %p, i128 0, i128 0 seq_cst seq_cst, align 16
  %loaded = extractvalue { i128, i1 } %1, 0
  ret i128 %loaded
}

define i128 @f2(ptr %p) #0 {
  %1 = cmpxchg ptr %p, i128 0, i128 0 seq_cst seq_cst, align 16
  %loaded = extractvalue { i128, i1 } %1, 0
  ret i128 %loaded
}

define i128 @f3(ptr %p) #0 {
  %1 = cmpxchg ptr %p, i128 0, i128 0 seq_cst seq_cst, align 16
  %loaded = extractvalue { i128, i1 } %1, 0
  ret i128 %loaded
}
```

All three functions produce identical output. f1 lost both `volatile` and
`syncscope("singlethread")`; f2 lost `syncscope("singlethread")`; f3 lost
`volatile`.

## Why this matters
- **Volatile** dropped: a volatile MMIO-style atomic load may now be elided by
  a later pass that thinks the cmpxchg is a non-volatile dummy 0->0 CAS.
- **Syncscope** dropped: `singlethread` is being widened to system. The
  resulting cmpxchg has stronger ordering than requested; conversely if
  the lowering for cmpxchg without singlethread emits weaker fences (target-
  dependent), a same-thread fastpath becomes incorrect.

The fix is two lines: pass `LI->getSyncScopeID()` to the CreateAtomicCmpXchg
call and call `cast<AtomicCmpXchgInst>(Pair)->setVolatile(LI->isVolatile())`.

## Related bugs
Sibling of w24 (widenPartwordAtomicRMW), w57 (LowerAtomic), bug-110 (lowerAtomic
similar pattern). Distinct call site.
