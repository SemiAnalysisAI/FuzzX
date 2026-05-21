# InstCombine `unpackStoreToAggregate` / `unpackLoadToAggregate` drops `!nontemporal` (cross-pass: makes downstream `!nontemporal` recovery impossible)

## Pattern

When `InstCombine`'s `visitStoreInst` (or `visitLoadInst`) sees a memory operation on an aggregate `{i32, i32}` (with no padding), it unpacks it into one scalar memory op per field. The new instructions are constructed with `IC.Builder.CreateAlignedStore` / `CreateAlignedLoad` and have only `setAAMetadata` (and `MD_invariant_load` on the load path) propagated. `!nontemporal` — which lives outside the AA metadata class — is silently dropped.

The store path at `InstCombineLoadStoreAlloca.cpp:1372-1381`:
```cpp
for (unsigned i = 0; i < Count; i++) {
  auto *Ptr = IC.Builder.CreateInBoundsPtrAdd(...);
  auto *Val = IC.Builder.CreateExtractValue(V, i, EltName);
  auto EltAlign = commonAlignment(Align, SL->getElementOffset(i)...);
  llvm::Instruction *NS = IC.Builder.CreateAlignedStore(Val, Ptr, EltAlign);
  NS->setAAMetadata(SI.getAAMetadata());  // <-- only AA. !nontemporal lost.
}
```

The array path at `InstCombineLoadStoreAlloca.cpp:1416-1428` has the same bug, and the load-side counterparts at `unpackLoadToAggregate` (lines 731-829) only thread `MD_invariant_load` in addition to AA.

The cross-pass amplifier is that once InstCombine has split one nontemporal `store {i32,i32}` into two scalar stores **without** the hint, no downstream pass (EarlyCSE, SimplifyCFG, DSE, GVN) has any way to re-derive the user intent — the metadata is permanently gone before later passes see the IR. Subsequent passes will happily DSE/forward/hoist these stores as ordinary temporal stores.

## .ll

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%struct.S = type { i32, i32 }

define void @unpack_store_nt(ptr %p, %struct.S %s) {
entry:
  store %struct.S %s, ptr %p, align 4, !nontemporal !0
  ret void
}

define %struct.S @unpack_load_nt(ptr %p) {
entry:
  %s = load %struct.S, ptr %p, align 4, !nontemporal !0
  ret %struct.S %s
}

!0 = !{i32 1}
```

## opt -O2 -S diff

Store path, after `-passes=instcombine` (which is enough; `-O2` produces the same output for these stores):
```llvm
define void @unpack_store_nt(ptr %p, %struct.S %s) {
entry:
  %s.elt = extractvalue %struct.S %s, 0
  store i32 %s.elt, ptr %p, align 4                  ; <-- !nontemporal GONE
  %p.repack1 = getelementptr inbounds nuw i8, ptr %p, i64 4
  %s.elt2 = extractvalue %struct.S %s, 1
  store i32 %s.elt2, ptr %p.repack1, align 4         ; <-- !nontemporal GONE
  ret void
}
```

Load path:
```llvm
define %struct.S @unpack_load_nt(ptr %p) {
entry:
  %s.unpack = load i32, ptr %p, align 4              ; <-- !nontemporal GONE
  %0 = insertvalue %struct.S poison, i32 %s.unpack, 0
  %s.elt1 = getelementptr inbounds nuw i8, ptr %p, i64 4
  %s.unpack2 = load i32, ptr %s.elt1, align 4        ; <-- !nontemporal GONE
  %s3 = insertvalue %struct.S %0, i32 %s.unpack2, 1
  ret %struct.S %s3
}
```

The original IR's only `!nontemporal` annotation disappears; `!0 = !{i32 1}` becomes unused metadata.

## Which pass triggers / which is at fault

- **Trigger**: `InstCombinerImpl::visitStoreInst` at `InstCombineLoadStoreAlloca.cpp:1465` calls `unpackStoreToAggregate` (line 1474). Similarly `visitLoadInst:1099` calls `unpackLoadToAggregate`.
- **Actual fault**: 
  - Store, struct path: `InstCombineLoadStoreAlloca.cpp:1372-1381`. The `for (unsigned i = 0; i < Count; i++)` loop calls `NS->setAAMetadata(SI.getAAMetadata())` only; should call `copyMetadata` for at least `MD_nontemporal`.
  - Store, array path: `InstCombineLoadStoreAlloca.cpp:1416-1428`. Same bug as struct path.
  - Load, struct path: `InstCombineLoadStoreAlloca.cpp:769-782`. Only AA and `MD_invariant_load` are copied; `MD_nontemporal` lost.
  - Compare with the single-element struct path at `InstCombineLoadStoreAlloca.cpp:1349-1352` which calls `combineStoreToNewValue` — that helper *does* copy `MD_nontemporal` correctly (see lines 628-648). The bug is *only* in the multi-element splitting paths.
- **Cross-pass amplifier**: Once split, no downstream pass can recover the lost annotation. `early-cse` and `simplifycfg` then operate on the metadata-less scalar memory ops, propagating ordinary cache-temporal behavior into the lowered machine code.

## Verified
- `opt -passes=instcombine -S` (with `instcombine<no-verify-fixpoint>` to be sure it's one iteration) on the store case drops `!nontemporal` on both unpacked stores.
- `opt -O2 -S` matches.
- Compare with `opt -O2 -S` of the same code without the aggregate — a plain `store i32` keeps `!nontemporal` through `-O2`.

## Source citations

- `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp:1334-1434` — `unpackStoreToAggregate` (struct + array paths, both buggy).
- `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp:1379-1381` — store struct path: only `setAAMetadata` after `CreateAlignedStore`.
- `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp:1425-1426` — store array path: same.
- `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp:777-782` — load struct path: only AA + `MD_invariant_load` copied.
- `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp:616-666` — `combineStoreToNewValue` (the *correct* helper) which iterates and threads `MD_nontemporal` through the switch at lines 628-663.

## Fix sketch

Replace the targeted `setAAMetadata` with a `copyMetadata` call that includes nontemporal (and ideally `mem_parallel_loop_access`, `access_group`, `noalias_addrspace`, anything `combineStoreToNewValue` handles):

```cpp
// Store struct path
for (unsigned i = 0; i < Count; i++) {
  ...
  StoreInst *NS = IC.Builder.CreateAlignedStore(Val, Ptr, EltAlign);
  NS->setAAMetadata(SI.getAAMetadata());
  NS->copyMetadata(SI, {LLVMContext::MD_nontemporal,
                        LLVMContext::MD_mem_parallel_loop_access,
                        LLVMContext::MD_access_group});
}
```

The cleanest fix would be to factor out the metadata-copy list shared with `combineStoreToNewValue` so it cannot drift again.
