# w66: AtomicExpandPass expandAtomicStoreToXChg drops volatile and syncscope

## Root cause
`AtomicExpandPass::expandAtomicStoreToXChg` rewrites an atomic store into an
`AtomicRMWInst::Xchg` (which downstream becomes a cmpxchg loop). The new RMW
never gets `setVolatile(SI->isVolatile())` and isn't passed `SI->getSyncScopeID()`.

```
llvm/lib/CodeGen/AtomicExpandPass.cpp:715
void AtomicExpandImpl::expandAtomicStoreToXChg(StoreInst *SI) {
  ...
  AtomicRMWInst *AI = Builder.CreateAtomicRMW(
      AtomicRMWInst::Xchg, SI->getPointerOperand(), SI->getValueOperand(),
      SI->getAlign(), RMWOrdering);
  // BUG: no AI->setVolatile(SI->isVolatile());
  // BUG: no SSID passed to CreateAtomicRMW; defaults to System.
  SI->eraseFromParent();
  tryExpandAtomicRMW(AI);
}
```

## Trigger condition (x86)
On x86_64 with `+cx16`, a 128-bit `store atomic` triggers this path.

```
target triple = "x86_64-unknown-linux-gnu"

define void @f1(ptr %p, i128 %x) {
  store atomic volatile i128 %x, ptr %p syncscope("singlethread") seq_cst, align 16
  ret void
}

define void @f2(ptr %p, i128 %x) {
  store atomic i128 %x, ptr %p syncscope("singlethread") seq_cst, align 16
  ret void
}

define void @f3(ptr %p, i128 %x) {
  store atomic volatile i128 %x, ptr %p seq_cst, align 16
  ret void
}
```

After `llc -mtriple=x86_64-unknown-linux-gnu -mattr=+cx16 -stop-after=atomic-expand`:

```
define void @f1(ptr %p, i128 %x) #0 {
  %1 = load i128, ptr %p, align 16                    ; <-- non-atomic, non-volatile!
  br label %atomicrmw.start
atomicrmw.start:
  %loaded = phi i128 [ %1, %0 ], [ %newloaded, %atomicrmw.start ]
  %2 = cmpxchg ptr %p, i128 %loaded, i128 %x seq_cst seq_cst, align 16   ; <-- no volatile, system syncscope
  ...
}
```

The same output is produced for f2 (no volatile in input) and f3 (no
syncscope in input): all three are bit-identical IR after the pass.

Two losses in one transform:
1. The seed `%1 = load i128, ptr %p, align 16` is a **bare non-atomic
   non-volatile load** synthesised from `tryExpandAtomicRMW`'s setup. It
   reads from MMIO storage that the user marked volatile.
2. The `cmpxchg` itself lost `volatile` and lost `syncscope("singlethread")`.

## Why this matters
MMIO/devicelist code that stores `volatile atomic i128` (e.g. to a 128-bit
hardware register) now has its store materialised as a non-volatile read of
the same address (which a side-effect-free helper or DCE can elide) and a
strong system-scope cmpxchg in place of the requested singlethread store.
This both inserts an unsolicited read AND drops the volatile contract on
the original store.

## Fix
Two lines, mirroring the well-formed `convertAtomicStoreToIntegerType` (line
697) and `convertAtomicXchgToIntegerType` (line 579): pass
`SI->getSyncScopeID()` to `CreateAtomicRMW` and add
`AI->setVolatile(SI->isVolatile())`.

## Related
Sibling of w66-atomic-expand-load-to-cmpxchg, w24 (widenPartword), w57
(LowerAtomic). Distinct call site.
