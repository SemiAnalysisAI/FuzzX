# w254 — LoopVectorize widens `volatile`/`atomic` loads/stores in `!llvm.loop.parallel_accesses` loop, silently dropping the qualifier

## Files / locations

- `llvm/lib/Analysis/LoopAccessAnalysis.cpp:2635-2641` and `:2659-2665`
  (legality gate that opens the door)
- `llvm/lib/Transforms/Vectorize/VPlanRecipes.cpp:3767-3771`,
  `:3859-3862` (load/store widening that produces the plain
  `CreateAlignedLoad`/`CreateAlignedStore` with no volatile/atomic
  carry-over)

## Bug

`LoopAccessLegality::canAnalyzeLoop` bypasses the simplicity check for
loads and stores whenever the loop is `IsAnnotatedParallel`:

```cpp
if (!Ld->isSimple() && !IsAnnotatedParallel) {           // line 2635
  recordAnalysis("NonSimpleLoad", Ld)
      << "read with atomic ordering or volatile read";
  ...
}
...
if (!St->isSimple() && !IsAnnotatedParallel) {           // line 2659
  recordAnalysis("NonSimpleStore", St)
      << "write with atomic ordering or volatile write";
  ...
}
```

So a `load volatile` or `load atomic unordered` inside a loop tagged
`!{!"llvm.loop.parallel_accesses", !group}` is accepted for
vectorization.  But `VPWidenLoadRecipe::execute` and
`VPWidenStoreRecipe::execute` create the wide replacement with the plain
`Builder.CreateAlignedLoad` / `Builder.CreateAlignedStore` — there is no
`setVolatile(true)` or atomic-ordering replay.

Net effect: a volatile or atomic-unordered scalar load/store is replaced
by a wide *non-volatile, non-atomic* vector load/store. The qualifier
the front-end emitted as a hard contract is silently erased.

## Reproducer (volatile)

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @f(ptr noalias %a, ptr noalias %b) #0 {
entry:
  br label %loop
loop:
  %iv  = phi i64 [0, %entry], [%iv.next, %loop]
  %p   = getelementptr i32, ptr %a, i64 %iv
  %v   = load volatile i32, ptr %p, align 4, !llvm.access.group !2
  %add = add nsw i32 %v, 7
  %q   = getelementptr i32, ptr %b, i64 %iv
  store i32 %add, ptr %q, align 4, !llvm.access.group !2
  %iv.next = add nuw nsw i64 %iv, 1
  %ec  = icmp eq i64 %iv.next, 1024
  br i1 %ec, label %exit, label %loop, !llvm.loop !0
exit:
  ret void
}
attributes #0 = { "target-features"="+avx2" }
!0 = distinct !{!0, !1}
!1 = !{!"llvm.loop.parallel_accesses", !2}
!2 = distinct !{}
```

Pipeline: `opt -mtriple=x86_64-unknown-linux-gnu -mattr=+avx2
-passes='loop-vectorize' -force-vector-width=4 -S` produces:

```llvm
vector.body:
  %wide.load = load <4 x i32>, ptr %0, align 4, !llvm.access.group !0   ; <-- NOT volatile
  ...
  %4 = add nsw <4 x i32> %wide.load, splat (i32 7)
  ...
```

Same at `-O2 -S` (`<8 x i32>` after IC=4 picks UF=4, still non-volatile).

## Reproducer (atomic unordered)

Identical loop with `%v = load atomic i32, ptr %p unordered, align 4` and
`store atomic ... unordered`. Result is plain non-atomic
`load <4 x i32>` / `store <4 x i32>`.

## Why this is wrong

- Volatile semantics in LangRef: "the optimizer must not change the
  number of volatile operations or change their order of execution
  relative to other volatile operations." Coalescing four scalar
  volatile loads into one vector load **eliminates three** of the four
  observable side effects and changes the access width seen by hardware
  (e.g. MMIO). Volatile must NEVER be silently dropped, regardless of
  any loop annotation.
- `!llvm.loop.parallel_accesses` is an assertion about iteration
  *independence* (no loop-carried memory dependence on members of the
  group). It does **not** authorize discarding `volatile` or weakening
  `atomic` to plain.
- For atomic-unordered the issue is two-fold:
  1. The widened vector load is not an atomic instruction at all
     (atomic vectors of size > 64-bit aren't legal in LLVM IR), so the
     access is no longer guaranteed atomic at element granularity. A
     concurrent writer could now produce a torn read that the original
     `load atomic unordered` was specified to never produce.
  2. Even if the wide access were atomic, the front-end requested
     element-wise atomicity, not access-wide atomicity.
- The legality gate at LAA:2635/2659 simply ignores volatile/atomic when
  the loop is parallel-marked; nothing downstream catches the mismatch.
  `VPWidenLoadRecipe::execute` doesn't even *check*
  `Ingredient.isVolatile()` /  `isAtomic()` before emitting a plain
  load.

## Fix sketch

The cheapest correctness fix: tighten the LAA gate to require
`isSimple()` regardless of `IsAnnotatedParallel`. Parallel-accesses
only meaningfully apply to memory dependences, not to
volatile/atomic semantics:

```cpp
if (!Ld->isSimple()) {
  recordAnalysis("NonSimpleLoad", Ld)
      << "read with atomic ordering or volatile read";
  HasComplexMemInst = true;
  continue;
}
```
(and symmetrically for stores at :2659).

A safer alternative is to keep the LAA loosening but reject in LV
legality / cost model — e.g. `canVectorizeMemory()` should refuse if
any in-loop memory access has `isVolatile()` or `!isUnordered()` /
atomic ordering. Or `VPWidenLoadRecipe::execute` should at least
preserve volatile via `cast<LoadInst>(NewLI)->setVolatile(true)` when
all members agree.
