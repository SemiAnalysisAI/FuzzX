# w57 — LowerAtomic drops `volatile` (and ordering/syncscope) on atomicrmw / cmpxchg

## Location

`llvm/lib/Transforms/Utils/LowerAtomic.cpp`:

- `buildCmpXchgValue` (lines 40-50) — uses `Builder.CreateAlignedLoad` /
  `CreateAlignedStore` with no volatile or ordering propagation.
- `lowerAtomicRMWInst` (lines 129-143) — uses `Builder.CreateLoad` /
  `CreateStore` with no volatile/ordering propagation. The original RMWI's
  `isVolatile()`, `getOrdering()`, `getSyncScopeID()` are all silently
  discarded.

Both helpers are invoked unconditionally on every RMW/cmpxchg in the
`lower-atomic` pass (`LowerAtomicPass.cpp` lines 45/47) when the user has
guaranteed a "non-preemptible environment". But the user did not guarantee
the operation is non-volatile — the `volatile` flag is independent of
preemption and means "do not eliminate / coalesce this memory access".

## Repro

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @vol_atomicrmw(ptr %p) {
  %old = atomicrmw volatile add ptr %p, i32 1 seq_cst, align 4
  ret i32 %old
}

define i32 @vol_cmpxchg(ptr %p) {
  %p1 = cmpxchg volatile ptr %p, i32 0, i32 1 seq_cst seq_cst, align 4
  %v = extractvalue { i32, i1 } %p1, 0
  ret i32 %v
}
```

## Invocation

```
opt -passes=lower-atomic -S input.ll
```

## Before/after diff

Before:
```
%old = atomicrmw volatile add ptr %p, i32 1 seq_cst, align 4
%p1  = cmpxchg   volatile ptr %p, i32 0, i32 1 seq_cst seq_cst, align 4
```

After (volatile dropped from both load AND store of each lowering):
```
%1   = load  i32, ptr %p, align 4    ; should be load volatile
%new = add   i32 %1, 1
       store i32 %new, ptr %p, align 4 ; should be store volatile

%1   = load  i32, ptr %p, align 4    ; should be load volatile
...
       store i32 %3, ptr %p, align 4  ; should be store volatile
```

The `volatile` qualifier in the source is silently lost. Volatile RMWs are
typically used for MMIO or signal-handler-visible state where every
load/store must be emitted.
