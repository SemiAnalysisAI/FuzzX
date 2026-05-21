# w638: `AtomicExpandImpl::expandAtomicLoadToCmpXchg` silently drops `syncscope`, `volatile`, and AA metadata when lowering `load atomic` to `cmpxchg`

## Severity
- **Syncscope drop**: Correctness/perf miscompile. A `singlethread` /
  `wavefront` / target-named scope is silently widened to system scope. On
  X86 this turns a thread-private 128-bit load into a real `lock cmpxchg16b`,
  which is observable to other threads and slower than necessary. On targets
  where scope determines what fences / cache writebacks the *backend* emits
  around the access, widening can also weaken thread-local guarantees in
  the wrong direction.
- **Volatile drop**: Semantic drop. The lowered cmpxchg no longer has
  observable-side-effect semantics that the original volatile atomic load
  required.
- **AA metadata drop**: Alias-analysis misinformation (same shape as w636).

## Source

`llvm/lib/CodeGen/AtomicExpandPass.cpp:668-687`

```cpp
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
      // <-- no SyncScope::ID arg -> defaults to SyncScope::System
      // <-- no NewCI->setVolatile(LI->isVolatile())
      // <-- no copyMetadataForAtomic(*NewCI, *LI)
  Value *Loaded = Builder.CreateExtractValue(Pair, 0, "loaded");

  LI->replaceAllUsesWith(Loaded);
  LI->eraseFromParent();

  return true;
}
```

For comparison, `createCmpXchgInstFun`
(`AtomicExpandPass.cpp:737-765`) - the helper used by the RMW path - does it
correctly:

```cpp
AtomicCmpXchgInst *Pair = Builder.CreateAtomicCmpXchg(
    Addr, Loaded, NewVal, AddrAlign, MemOpOrder,
    AtomicCmpXchgInst::getStrongestFailureOrdering(MemOpOrder), SSID);
Pair->setVolatile(IsVolatile);
if (MetadataSrc)
  copyMetadataForAtomic(*Pair, *MetadataSrc);
```

It plumbs `SSID`, `IsVolatile`, and `MetadataSrc` through. The load-to-cmpxchg
helper has no analog.

`IRBuilder::CreateAtomicCmpXchg` with the SSID argument omitted defaults to
`SyncScope::System`:

```cpp
// IRBuilder.h
AtomicCmpXchgInst *CreateAtomicCmpXchg(
    Value *Ptr, Value *Cmp, Value *New, MaybeAlign Align,
    AtomicOrdering SuccessOrdering, AtomicOrdering FailureOrdering,
    SyncScope::ID SSID = SyncScope::System);
```

## Repro 1 - `syncscope("singlethread")` silently widened to system scope

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i128 @load_i128_singlethread(ptr %p) {
  %v = load atomic volatile i128, ptr %p syncscope("singlethread") seq_cst, align 16
  ret i128 %v
}
```

```console
$ llc -mtriple=x86_64-unknown-linux-gnu -mattr=+cx16,-avx -stop-after=atomic-expand repro.ll -o -
...
define i128 @load_i128_singlethread(ptr %p) #0 {
  %1 = cmpxchg ptr %p, i128 0, i128 0 seq_cst seq_cst, align 16
                                                    ; ^^ no syncscope("singlethread")
                                                    ; ^^ no volatile
  %loaded = extractvalue { i128, i1 } %1, 0
  ret i128 %loaded
}
```

The input has `syncscope("singlethread")` and `volatile`; the produced cmpxchg
has neither. The cmpxchg implicitly defaults to `syncscope("system")`.

Final assembly on `x86_64-linux-gnu -mattr=+cx16,-avx`:

```asm
load_i128_singlethread:
    ...
    lock cmpxchg16b (%rdi)     ; lock prefix - unnecessary for singlethread
    ...
```

A correctly-scoped singlethread cmpxchg on x86_64 still needs `cmpxchg16b`
for atomicity of the 128-bit access on a single thread (interrupt safety),
but does *not* need `lock` (no cross-thread bus contention). The implicit
widening defeats that optimization opportunity in lowering.

## Repro 2 - `!tbaa` lost on the lowered cmpxchg

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i128 @load_i128_tbaa(ptr %p) {
  %v = load atomic i128, ptr %p seq_cst, align 16, !tbaa !0
  ret i128 %v
}

!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C++ TBAA"}
```

```console
$ llc -mtriple=x86_64-unknown-linux-gnu -mattr=+cx16,-avx -stop-after=atomic-expand repro.ll -o -
define i128 @load_i128_tbaa(ptr %p) #0 {
  %1 = cmpxchg ptr %p, i128 0, i128 0 seq_cst seq_cst, align 16   ; !tbaa lost
  %loaded = extractvalue { i128, i1 } %1, 0
  ret i128 %loaded
}
```

## Suggested fix

Pass the source instruction's SyncScope, volatility, and metadata through:

```cpp
AtomicCmpXchgInst *Pair = Builder.CreateAtomicCmpXchg(
    Addr, DummyVal, DummyVal, LI->getAlign(), Order,
    AtomicCmpXchgInst::getStrongestFailureOrdering(Order),
    LI->getSyncScopeID());
Pair->setVolatile(LI->isVolatile());
copyMetadataForAtomic(*Pair, *LI);
```

(The same fix shape is needed in the load-to-LL/SC paths
`expandAtomicLoadToLL` and `expandAtomicOpToLLSC` for load, both of which
also propagate via `LI->getOrdering()` only.)

## Cross-reference

This is a related but distinct defect from:
- **w635**: same function emits IR with the original load's type (which can
  be a vector and crash the verifier);
- **w636**: sister `convertAtomic{Load,Store,CmpXchg}ToIntegerType` helpers
  drop AA metadata.

All three sit in `AtomicExpandPass.cpp` and share the underlying gap: there
is no shared "clone-an-atomic-cmpxchg-from-load" helper, so every callsite
that synthesizes a cmpxchg has to remember which subset of source-instruction
properties to propagate, and `expandAtomicLoadToCmpXchg` forgets most of
them.
