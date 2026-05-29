# w100 - VectorCombine `scalarizeLoad` infinite loop on stronger-than-unordered atomic

> **Status:** Fixed by PR [#200263](https://github.com/llvm/llvm-project/pull/200263)
> (open), tracked on branch `fix3-191`. Bugs **148, 190, and 191** are all the
> same root cause and are all resolved by the single `isVolatile()` →
> `!isSimple()` gate change. The `!isSimple()` gate rejects the atomic load up
> front, preventing both the IR miscompile (148/190) and this infinite loop.

## Location

`llvm/lib/Transforms/Vectorize/VectorCombine.cpp`

- Entry gate: `VectorCombine::scalarizeLoad` line 2008-2059
- Both downstream paths affected: `scalarizeLoadBitcast` (line 2150) and
  `scalarizeLoadExtract` (line 2063).

The entry gate at line 2015 filters only `isVolatile()`:

```cpp
if (LI->isVolatile() || !DL->typeSizeEqualsStoreSize(VecTy->getScalarType()))
  return false;
```

When the input is `load atomic <N x T>` with ordering stronger than
`unordered` (`monotonic` / `acquire` / `seq_cst`) and the users are all
extractelements or all bitcasts, the transform:

1. Creates a new non-atomic scalar load (`Builder.CreateLoad` at line 2202
   for bitcast, line 2130 for extract).
2. `replaceValue(*BC, ScalarLoad, /*Erase=*/false)` — replaces uses of the
   bitcast/extract but does NOT erase the original vector load.
3. The original atomic vector load is pushed back to the worklist
   (`Worklist.push(LI)` line 2198 / 2112).

Now the vector load has only "dead" users (bitcasts/extracts whose uses are
all replaced). On the NEXT worklist visit, `scalarizeLoad` is re-entered.
Line 2033 (`if (UI->use_empty()) return false;`) bails — but the worklist
keeps re-pushing the load because **for ordering > unordered the original
load is not trivially dead** (`mayHaveSideEffects()` returns true due to
the synchronization ordering), so the `eraseInstruction` path is never
reached. The dead bitcast / extract users are also never erased.

Each iteration: vector-combine sees the vector load, re-enters scalarizeLoad,
attempts to rescan users, bails. The worklist `pushUsersToWorkList` /
`pushValue(NewI)` from the new scalar load and the (kept) old load triggers
fresh visits, hitting the cost-model fixed point but never converging — opt
spins at 100% CPU forever.

For `unordered` atomic the same code path runs once, but the original
vector load IS trivially dead (`isUnordered() && !isVolatile()` → no side
effects) so `eraseInstruction` removes it and the worklist drains. That
hides the bug for unordered; stronger orderings expose it.

## Repro (`repro.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

; Hang via bitcast path (scalarizeLoadBitcast)
define i64 @hang_bitcast(ptr %p) {
  %v  = load atomic <2 x i32>, ptr %p acquire, align 8
  %bc = bitcast <2 x i32> %v to i64
  ret i64 %bc
}

; Hang via extract path (scalarizeLoadExtract)
define i32 @hang_extract(ptr %p) {
  %v = load atomic <2 x i32>, ptr %p seq_cst, align 8
  %e = extractelement <2 x i32> %v, i32 0
  ret i32 %e
}
```

## Invocation

```
timeout 5 opt -mtriple=x86_64 -passes=vector-combine -S repro.ll
```

## Observed behavior

`opt` hangs indefinitely consuming 100% CPU; `timeout 5` kills it (exit
status 124/143). Neither function ever produces output. Reproduces with
`monotonic`, `acquire`, `release`, `acq_rel`, `seq_cst`. Does NOT reproduce
with `unordered` (because the original load is then trivially dead and
gets erased after the first transform) and does NOT reproduce with
`load atomic volatile` (because `isVolatile()` filters it at the gate).

## opt diff vs unordered

```
$ timeout 5 opt -passes=vector-combine -S repro_unordered.ll
... terminates with non-atomic load (the w100-strips-atomic bug)

$ timeout 5 opt -passes=vector-combine -S repro_seq_cst.ll
Terminated   (exit 143)
```

## Pipeline note (-O2)

At `-O2`, this hang manifests whenever a `bitcast (load atomic <N x T>)`
or `extractelement (load atomic <N x T>)` shape reaches vector-combine
without instcombine first canonicalizing it. The default pipeline runs
instcombine before vector-combine, but multi-use bitcasts, indirect
predecessor passes, and certain pre-canonicalized inputs can keep this
shape alive — e.g. when the bitcast has two distinct integer scalar uses,
instcombine's bitcast-load fold may not fire and vector-combine then sees
the unfolded shape, hanging the whole compilation.

## Fix

Same as w100-bitcast-strips-atomic: gate at line 2015 must require
`LI->isSimple()`, not just `!LI->isVolatile()`. That single-line change
prevents both the IR-level miscompile AND the infinite loop, because the
load with non-unordered ordering will be rejected up front and never enter
the worklist re-visit cycle.

```cpp
- if (LI->isVolatile() || !DL->typeSizeEqualsStoreSize(...))
+ if (!LI->isSimple() || !DL->typeSizeEqualsStoreSize(...))
    return false;
```
