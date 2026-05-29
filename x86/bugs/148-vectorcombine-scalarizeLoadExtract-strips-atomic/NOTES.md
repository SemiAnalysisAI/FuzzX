# w63b - VectorCombine `scalarizeLoadExtract` drops `atomic` ordering on vector load

> **Status:** Fixed by PR [#200263](https://github.com/llvm/llvm-project/pull/200263)
> (open), tracked on branch `fix3-191`. Bugs **148, 190, and 191** are all the
> same root cause and are all resolved by the single `isVolatile()` →
> `!isSimple()` gate change. No separate fix needed for this entry.

## Location

`llvm/lib/Transforms/Vectorize/VectorCombine.cpp` line 2008
(`scalarizeLoad` → `scalarizeLoadExtract`).

The entry gate at line 2015 only rejects `isVolatile`:

```cpp
if (LI->isVolatile() || !DL->typeSizeEqualsStoreSize(VecTy->getScalarType()))
  return false;
```

It does NOT check `LI->isAtomic()`. Atomic vector loads then fall through
to line 2130, which emits plain non-atomic scalar loads via
`Builder.CreateLoad(ElemType, GEP, ...)`. The atomicity guarantee
(no-torn-read, ordering w.r.t. other atomic ops) is silently discarded.

```cpp
auto *NewLoad = cast<LoadInst>(
    Builder.CreateLoad(ElemType, GEP, EI->getName() + ".scalar"));
```

A single `load atomic unordered <4 x i32>` becomes N independent
non-atomic scalar loads of i32. Each scalar load can interleave with
concurrent writes by other threads, breaking the no-torn-read guarantee
the original `load atomic` provided. The transform is also illegal even
for `unordered`, since the original is a single observable atomic event
in the C/C++/LLVM memory model.

The companion `scalarizeLoadBitcast` (called from the same `scalarizeLoad`
entry, lines 2057-2058) has the same defect — its load-creating helper
also uses `Builder.CreateLoad(...)` without an ordering argument
(line 2202).

## Repro (`repro.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define i32 @vec_extract_atomic(ptr %p) {
  %v  = load atomic <4 x i32>, ptr %p unordered, align 16
  %e0 = extractelement <4 x i32> %v, i32 0
  %e1 = extractelement <4 x i32> %v, i32 1
  %s  = add i32 %e0, %e1
  ret i32 %s
}
```

## Invocation

```
opt -passes=vector-combine -S repro.ll
```

Also reproduces with `-O3` since vector-combine runs as part of the
default optimization pipeline.

## Observed output

```
define i32 @vec_extract_atomic(ptr %p) {
  %e0 = load i32, ptr %p, align 16          ; <-- atomic dropped
  %1  = getelementptr inbounds <4 x i32>, ptr %p, i32 0, i32 1
  %e1 = load i32, ptr %1, align 4           ; <-- atomic dropped
  %s  = add i32 %e0, %e1
  ret i32 %s
}
```

The two scalar i32 loads are non-atomic, so on x86 the codegen is just
`mov`/`mov`, which a concurrent writer can tear (or worse, interleave a
write between the two scalar loads, producing an i32 pair that never
existed in the original atomic vector value).

## Fix

Two related fixes:

1. Tighten the entry gate to require `LI->isSimple()`:
   ```cpp
   if (!LI->isSimple() || !DL->typeSizeEqualsStoreSize(...))
     return false;
   ```
2. (Alternative) Propagate the original ordering/syncscope onto each
   created scalar load. But splitting one atomic load into N atomic loads
   is itself unsound for orderings stronger than `unordered`, so option
   (1) is the correct fix.

## Family

Same defect class as bugs 108 / 109 / 111 / 114 / w63b-matrix —
optimization passes that emit IR with the default-non-volatile /
default-non-atomic IRBuilder helpers and forget to propagate the
volatile/atomic bit from the original IR. VectorCombine's check looks
correct on the surface (`isVolatile()`) but is incomplete — `isSimple()`
is required.
