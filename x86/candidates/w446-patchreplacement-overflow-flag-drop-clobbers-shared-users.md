# `patchReplacementInstruction`: dropping poison-generating flags on `ReplInst` when replacing `extractvalue 0` from a `*.with.overflow` clobbers UNRELATED users of `ReplInst`

**Pass surface:** `gvn` (and any other pass that calls `patchReplacementInstruction`).
**Source:** `llvm/lib/Transforms/Utils/Local.cpp` lines 3187-3198:
```cpp
WithOverflowInst *UnusedWO;
// When replacing the result of a llvm.*.with.overflow intrinsic with a
// overflowing binary operator, nuw/nsw flags may no longer hold.
if (isa<OverflowingBinaryOperator>(ReplInst) &&
    match(I, m_ExtractValue<0>(m_WithOverflowInst(UnusedWO))))
  ReplInst->dropPoisonGeneratingFlags();
```
**Triple:** `x86_64-unknown-linux-gnu`
**Tool:** `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -S -passes=gvn` (also `-O2`).

## Root cause

When GVN CSEs `extractvalue (sadd.with.overflow x y), 0` with an existing `%addnsw = add nsw x, y`, `patchReplacementInstruction(I=extractvalue, Repl=%addnsw)` correctly observes that the EV0 has no overflow guarantee, so it strips `nsw`/`nuw` from `%addnsw` to make the kept value safe to use at the EV's use site.

The bug: `dropPoisonGeneratingFlags` operates on `ReplInst` (the SURVIVING dominator value), not on the use-site. So the flag drop is **global** to `ReplInst`. Pre-existing users of `%addnsw` that legitimately relied on `nsw` to be guaranteed-non-overflow now see a flagless `add`. This is a refinement (poison → defined value), but is a *destructive* refinement applied to instructions that were not actually CSE'd.

The correct repair is one of:
- Add a `freeze` around the use-site of `Repl` from `I`'s users, then leave `ReplInst`'s flags intact.
- Drop the flags only on a CLONE of `ReplInst` inserted at I's location (defeats the purpose, but preserves users).

## Reproducer

```llvm
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare {i32, i1} @llvm.sadd.with.overflow.i32(i32, i32)
declare void @use1(i32)
declare void @use2(i32)

define void @f(i32 %x, i32 %y) {
entry:
  %addnsw = add nsw i32 %x, %y
  %wo = call {i32, i1} @llvm.sadd.with.overflow.i32(i32 %x, i32 %y)
  %ev = extractvalue {i32, i1} %wo, 0
  call void @use1(i32 %addnsw)
  call void @use2(i32 %ev)
  ret void
}
```

```
$ opt -S -passes=gvn repro.ll
```

After:
```
entry:
  %addnsw = add i32 %x, %y     ; <-- nsw lost!
  call void @use1(i32 %addnsw)
  call void @use2(i32 %addnsw)
  ret void
}
```

`%addnsw` had a legitimate `nsw` flag (e.g., front-end emitted from a signed arithmetic with proved no-overflow). After GVN CSE, the `nsw` is silently dropped because of an *unrelated* `extractvalue 0` of a `sadd.with.overflow`. `%use1`'s consumption — which existed pre-CSE and benefitted from `nsw` — is now flag-less. The same effect persists under `opt -O2`.

## Why this matters downstream

After GVN, downstream passes (e.g., InstCombine, LSR, SCEV) cannot prove `%addnsw` is non-overflowing anymore. Any reasoning over `%use1`'s consumer that hinges on `nsw` (loop-trip-count, gep-inbounds derivation, range narrowing) is silently weakened. The bug is invisible to a single-pass IR diff because the original `nsw` is just gone — no warning, no remark.

## Verification

Compare `opt -S -passes=gvn repro.ll` against the input: `add nsw → add`. A correct implementation would emit either:
- Pre-CSE `ev` left as-is (no CSE).
- A new `add` instruction created at the EV use, leaving `%addnsw nsw` intact.
- A `freeze (add nsw %x, %y)` substituted into `use2`.

LLVM's current code does none of these.

## Notes

- The `else if (!isa<LoadInst>(I)) ReplInst->andIRFlags(I);` branch on line 3197 also intersects flags onto `ReplInst`, with the same destructive scope.
- This is referenced by the FIXME on line 3211-3219 which acknowledges the conservative-merge problem for noalias scopes, but the same destructiveness applies to poison flags.
