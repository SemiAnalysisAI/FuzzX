# DSE `tryFoldIntoCalloc` drops `!annotation` and `!prof` from malloc + memset

**Component:** llvm/lib/Transforms/Scalar/DeadStoreElimination.cpp

**Function:** `DSEState::tryFoldIntoCalloc(MemoryDef*, const Value*)`.

**Lines:** 2252-2354. New `calloc` call built at 2316-2342 (either via the
`alloc-variant-zeroed` attribute path 2319-2337, or via `emitCalloc`
2338-2342). Original `Malloc` replaced/deleted at 2351-2352:
```
Malloc->replaceAllUsesWith(Calloc);
deleteDeadInstruction(Malloc);
```
The `MemSet` itself is killed by the normal `eliminateDeadDefs` flow after
the calloc is in place. Neither the new `Calloc` instruction nor any other
surviving instruction inherits metadata from `Malloc` or `MemSet`.

## Pattern

DSE recognizes `p = malloc(N); memset(p, 0, N);` and fuses it into a
`calloc(1, N)` call. The new call is built fresh — only the calling
convention and attribute list of `InnerCallee` (the declaration of the
zeroed allocator) are copied. Per-call metadata (`!annotation`, `!prof`,
`!callees`, etc.) attached to the original `Malloc` or to the `MemSet` is
not transferred to the new `Calloc` call.

## Bug

In the `alloc-variant-zeroed` path (2319-2337):
```cpp
CallInst *CI = IRB.CreateCall(ZeroedVariant, Args, ZeroedVariantName);
CI->setCallingConv(Malloc->getCallingConv());
Calloc = CI;
```
And in the libc-`calloc` path (2338-2342) `emitCalloc` builds a fresh
`CallInst` without copying metadata from `Malloc` or `MemSet`.

After the new call is in place, `Malloc->replaceAllUsesWith(Calloc)` is
called and `deleteDeadInstruction(Malloc)` runs — but no metadata combine.
`MemSet` is then killed by the subsequent `eliminateDeadDefs`, losing its
metadata as well.

## Confirmed via `opt -passes=dse` (x86_64, default `-O2`)

### Input IR
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare noalias ptr @malloc(i64) nounwind allockind("alloc,uninitialized") allocsize(0)
declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)

define ptr @f() {
  %p = call ptr @malloc(i64 16), !annotation !0, !prof !1
  call void @llvm.memset.p0.i64(ptr align 1 %p, i8 0, i64 16, i1 false), !annotation !2
  ret ptr %p
}

!0 = !{!"malloc_call"}
!1 = !{!"branch_weights", i32 100}
!2 = !{!"memset_call"}
```

### After `opt -passes=dse -S`
```
define ptr @f() {
  %calloc = call ptr @calloc(i64 1, i64 16)
  ret ptr %calloc
}
```

The new `%calloc` call carries **no** `!annotation` and **no** `!prof`.
The original `malloc` had both; the original `memset` had another
`!annotation`. All three pieces of metadata are silently lost.

Default x86 `-O2` shows the same: `tail call dereferenceable_or_null(16)
ptr @calloc(i64 1, i64 16)` with no `!annotation` or `!prof`.

## Impact

`!prof` loss is the more consequential one here — PGO-derived branch /
call frequencies attached to the original allocation site disappear after
calloc folding, degrading downstream PGO-driven decisions (inlining, code
placement). `!annotation` loss matches the family of w395-w398 deletion
sites.

## Fix sketch

After building the new `Calloc` (line 2342) and before
`Malloc->replaceAllUsesWith(Calloc)` at 2351, copy/combine metadata from
`Malloc` and `MemSet` onto `cast<Instruction>(Calloc)`. The standard
idiom is `combineMetadataForCSE(Calloc, Malloc, /*DoesKMove=*/false)`
followed by a similar combine with `MemSet`. At minimum `!prof` and
`!annotation` should be transferred.
