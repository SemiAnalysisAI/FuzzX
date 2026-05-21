# CGP splitMergedValStore: atomic store split into two non-atomic stores

File: `llvm/lib/CodeGen/CodeGenPrepare.cpp:8568-8665` (function `splitMergedValStore`).

## Reasoning

`splitMergedValStore` recognizes the pattern
`store i64 (or (zext lo), (shl (zext hi), 32)), ptr` and rewrites it into two
half-width stores (low and high). The bailout at line 8590 only rejects
volatile stores:

```cpp
// Don't split the store if it is volatile.
if (SI.isVolatile())
  return false;
```

It never queries `SI.isAtomic()` (nor `SI.getOrdering()`). The two replacement
stores are produced via `Builder.CreateAlignedStore(...)` which creates plain,
non-atomic stores with `NotAtomic` ordering. Therefore an atomic i64 store
that happens to have the bit-merge pattern as its value operand is silently
turned into two independent non-atomic stores, destroying atomicity and the
release/acquire ordering of the original.

On x86, `X86TargetLowering::isMultiStoresCheaperThanBitsMerge` returns true
when the (low, high) types are a mix of float and integer, which is exactly
what the IR fuzzer pattern below produces, so the transform fires in -O2
codegen.

For comparison, the sibling helper at the load side and other CGP store
rewriters consistently check `isAtomic()` (see e.g. the volatile/atomic
checks in `OptimizeNoopCopyExpression`, the SROA-style helpers, and DAG
combiner's `SimplifyDemandedBits` for stores). Missing the atomic check here
is a long-standing oversight that has been re-introduced before
(see commits around D154814 / GH-issue 79236 for the load side of the
same family).

## IR repro

Run with:

```
opt -passes=codegenprepare -mtriple=x86_64-unknown-linux-gnu -S < repro.ll
```

```llvm
; Atomic i64 store whose value happens to be (zext i32 %lo) | (zext-shl i32-bitcast-of-float %hi << 32)
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @split_atomic(ptr %p, i32 %lo, float %hf) {
entry:
  %hi.i = bitcast float %hf to i32
  %lo.z = zext i32 %lo  to i64
  %hi.z = zext i32 %hi.i to i64
  %hi.s = shl i64 %hi.z, 32
  %v    = or  i64 %lo.z, %hi.s
  ; ATOMIC release store of i64
  store atomic i64 %v, ptr %p release, align 8
  ret void
}
```

## Expected wrong outcome

After CGP the single `store atomic ... release` is replaced by two plain
non-atomic 32-bit stores (one for `%lo`, one for the bitcast of `%hf`),
e.g.:

```
  store i32 %lo, ptr %p, align 8
  %p1 = getelementptr i32, ptr %p, i32 1
  store i32 %hi.i, ptr %p1, align 4
  ret void
```

This is incorrect on three counts:

1. Atomicity of the original 8-byte store is gone — another thread can now
   observe a half-updated value.
2. The `release` ordering edge is dropped (the IR-level memory-model contract
   is broken; downstream the SDAG will not insert any fence).
3. Even single-thread, this violates the requirement that an atomic store be
   emitted as an indivisible store at the target level (x86 guarantees aligned
   8-byte stores are atomic — the two split 4-byte stores are not).

The fix is a one-line bailout next to the volatile check:

```cpp
if (SI.isVolatile() || SI.isAtomic())
  return false;
```
