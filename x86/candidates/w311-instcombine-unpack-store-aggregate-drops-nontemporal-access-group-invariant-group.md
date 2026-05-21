# w311: InstCombine `unpackStoreToAggregate` (struct AND array multi-element branches) drops `!nontemporal`, `!access_group`, `!mem_parallel_loop_access`, `!invariant.group`

## File / function

`llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp`,
`unpackStoreToAggregate` (called from `visitStoreInst` line 1474):

- STRUCT branch with `Count > 1`: lines 1346-1383, loop body at 1372-1381.
- ARRAY branch with `NumElements > 1`: lines 1386-1430, loop body at 1416-1428.

## Root cause

Both multi-element branches synthesize one new `StoreInst` per element
and copy only AA metadata:

```cpp
// struct branch
for (unsigned i = 0; i < Count; i++) {
  auto *Ptr = IC.Builder.CreateInBoundsPtrAdd(
      Addr, IC.Builder.CreateTypeSize(IdxType, SL->getElementOffset(i)),
      AddrName);
  auto *Val = IC.Builder.CreateExtractValue(V, i, EltName);
  auto EltAlign =
      commonAlignment(Align, SL->getElementOffset(i).getKnownMinValue());
  llvm::Instruction *NS = IC.Builder.CreateAlignedStore(Val, Ptr, EltAlign);
  NS->setAAMetadata(SI.getAAMetadata());   // <<< only AA copied
}
```

`setAAMetadata` only handles `{tbaa, tbaa_struct, alias_scope, noalias}`.
Every other store-applicable metadata kind is silently dropped on the
per-element stores:

- `!nontemporal`
- `!access_group`
- `!mem_parallel_loop_access`
- `!invariant.group`
- `!noalias_addrspace`
- `!prof`, `!fpmath`, `!DIAssignID`

This is the store-side mirror of w310. The single-element fast-paths
(lines 1351, 1391) go through `combineStoreToNewValue`, which has its
*own* incomplete switch (covered by w106-store-bitcast-drops-invariant-group
and w106-store-bitcast-drops-noalias-addrspace) but does at least handle
the `nontemporal / access_group / mem_parallel_loop_access` group.

## Reproducer 1 (struct multi-element): `!nontemporal` lost

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%S = type { i32, i32 }

define void @f(ptr %p, %S %v) {
  store %S %v, ptr %p, align 4, !nontemporal !0
  ret void
}

!0 = !{i32 1}
```

`opt -passes=instcombine -S`:

```llvm
define void @f(ptr %p, %S %v) {
  %v.elt = extractvalue %S %v, 0
  store i32 %v.elt, ptr %p, align 4                  ; !nontemporal gone
  %p.repack1 = getelementptr inbounds nuw i8, ptr %p, i64 4
  %v.elt2 = extractvalue %S %v, 1
  store i32 %v.elt2, ptr %p.repack1, align 4         ; !nontemporal gone
  ret void
}
```

## Reproducer 2 (array multi-element): `!nontemporal` + `!access_group` lost

```llvm
define void @f(ptr %p, [2 x i32] %v) {
  store [2 x i32] %v, ptr %p, align 4, !nontemporal !0, !access_group !1
  ret void
}

!0 = !{i32 1}
!1 = distinct !{}
```

Output:

```llvm
define void @f(ptr %p, [2 x i32] %v) {
  %v.elt = extractvalue [2 x i32] %v, 0
  store i32 %v.elt, ptr %p, align 4                  ; both gone
  %p.repack1 = getelementptr inbounds nuw i8, ptr %p, i64 4
  %v.elt2 = extractvalue [2 x i32] %v, 1
  store i32 %v.elt2, ptr %p.repack1, align 4         ; both gone
  ret void
}
```

## Reproducer 3 (array multi-element): `!invariant.group` lost

```llvm
define void @f(ptr %p, [2 x i32] %v) {
  store [2 x i32] %v, ptr %p, align 4, !invariant.group !0
  ret void
}
!0 = !{}
```

Output: same `extractvalue`/`store` skeleton with no `!invariant.group` on
either store. Same outcome for the equivalent `%S = { i32, i32 }` input.

All three reproduced against
`/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt`.

## Why it matters

- `!nontemporal` loss converts streaming stores into normal stores —
  measurable cache-pollution / perf regression for `__builtin_nontemporal_store`
  applied to struct types.
- `!access_group` / `!mem_parallel_loop_access` loss breaks
  LoopVectorize's ability to reorder these stores; loops containing
  whole-struct stores silently fail to vectorize after InstCombine.
- `!invariant.group` loss on stores is the documented vptr-substitution
  miscompile class (see w106). A struct or array store of a vtable-bearing
  aggregate that the frontend marks `!invariant.group` will lose the
  marker on every element store, then GVN/loadCSE may forward a peer
  `load !invariant.group` across the now-unmarked element store - the
  same miscompile as w106 but reached via the aggregate-store path
  instead of the bitcast-unwrap path.

## Fix shape

Replace each `NS->setAAMetadata(SI.getAAMetadata());` with a routine
that copies the full store-applicable metadata set (analogous to
`copyMetadataForLoad` but for stores). The existing
`combineStoreToNewValue` switch on lines 639-663 is the closest template
— a refactor to share that switch with the unpack loops is the
straightforward fix. (Also fix the missing entries from w106 in that
shared helper.)

## Confidence

High (all three reproducers verified).
Net-new file/function not previously covered: `unpackStoreToAggregate`
is referenced in no other w-prefixed candidate. Distinct from
w106 which fires on the `combineStoreToNewValue` bitcast-unwrap path.
