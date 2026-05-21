# w657: mergeicmps drops `!nontemporal` on merged memcmp call

- **File:** `llvm/lib/Transforms/Scalar/MergeICmps.cpp`
- **Target:** x86_64, LLVM 23.0.0git, default `-O2` lowering path
- **Pass:** `mergeicmps`
- **Severity:** Missed optimization (cache-bypass hint lost for streaming compare loops; the user's `!nontemporal` annotation is silently dropped)

## Root cause

Same locus as w655 but specifically about `!nontemporal`. The
multi-compare branch in `mergeComparisons` (`MergeICmps.cpp:690-707`)
calls `emitMemCmp` and never propagates `!nontemporal` from any of the
source loads to the resulting call or, importantly, to the loads that
`ExpandMemCmpPass` will later synthesize from this call.

```cpp
} else {
  const unsigned TotalSizeBits = std::accumulate(
      Comparisons.begin(), Comparisons.end(), 0u,
      [](int Size, const BCECmpBlock &C) { return Size + C.SizeBits(); });

  // memcmp expects a 'size_t' argument and returns 'int'.
  unsigned SizeTBits = TLI.getSizeTSize(*Phi.getModule());
  unsigned IntBits = TLI.getIntSize();

  // Create memcmp() == 0.
  const auto &DL = Phi.getDataLayout();
  Value *const MemCmpCall = emitMemCmp(           // <-- no metadata propagation
      Lhs, Rhs,
      ConstantInt::get(Builder.getIntNTy(SizeTBits), TotalSizeBits / 8),
      Builder, DL, &TLI);
  IsEqual = Builder.CreateICmpEQ(
      MemCmpCall, ConstantInt::get(Builder.getIntNTy(IntBits), 0));
}
```

The single-compare path at `MergeICmps.cpp:683-684` clones the original
load, which *does* preserve `!nontemporal`:

```cpp
Instruction *const LhsLoad = Builder.Insert(FirstCmp.Lhs().LoadI->clone());
Instruction *const RhsLoad = Builder.Insert(FirstCmp.Rhs().LoadI->clone());
```

So the bug is path-asymmetric: a 1-compare "merge" preserves
`!nontemporal`; a 2+-compare merge silently strips it.

## Repro

`/tmp/mergeicmps/nontemporal_drop.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%S = type { i32, i32 }

define i1 @eq(ptr dereferenceable(8) %a, ptr dereferenceable(8) %b) {
entry:
  %a0 = load i32, ptr %a, align 4, !nontemporal !0
  %b0 = load i32, ptr %b, align 4, !nontemporal !0
  %c0 = icmp eq i32 %a0, %b0
  br i1 %c0, label %rhs, label %end

rhs:
  %ag1 = getelementptr inbounds %S, ptr %a, i64 0, i32 1
  %bg1 = getelementptr inbounds %S, ptr %b, i64 0, i32 1
  %a1 = load i32, ptr %ag1, align 4, !nontemporal !0
  %b1 = load i32, ptr %bg1, align 4, !nontemporal !0
  %c1 = icmp eq i32 %a1, %b1
  br label %end

end:
  %r = phi i1 [ false, %entry ], [ %c1, %rhs ]
  ret i1 %r
}

!0 = !{i32 1}
```

## Diff (`opt -passes=mergeicmps -S`)

```
-  %a0 = load i32, ptr %a, align 4, !nontemporal !0
-  %b0 = load i32, ptr %b, align 4, !nontemporal !0
-  ...
-  %a1 = load i32, ptr %ag1, align 4, !nontemporal !0
-  %b1 = load i32, ptr %bg1, align 4, !nontemporal !0
-  %c1 = icmp eq i32 %a1, %b1
+  %memcmp = call i32 @memcmp(ptr %a, ptr %b, i64 8)
+  %0 = icmp eq i32 %memcmp, 0
```

All four `!nontemporal !0` annotations are gone. After
`ExpandMemCmpPass` inlines the memcmp into two `i64` loads on x86, those
loads are emitted as ordinary `movq`, not `movntdqa`.

## Why this matters on x86 -O2

The user marks loads `!nontemporal` to advise the backend to use the
streaming load family (`movntdqa` on SSE4.1+). After mergeicmps strips
the hint, the subsequently inlined memcmp loads emit regular `movq` and
pollute the L1 cache — a real-world performance regression on hot
streaming-compare paths (e.g., dedup checksums).

## Suggested fix

In `mergeComparisons` after creating the call, propagate `!nontemporal`
if *all* contributing source loads carry it (conservative union):

```cpp
bool AllNT = true;
for (const BCECmpBlock &C : Comparisons) {
  if (!C.Lhs().LoadI->getMetadata(LLVMContext::MD_nontemporal) ||
      !C.Rhs().LoadI->getMetadata(LLVMContext::MD_nontemporal)) {
    AllNT = false;
    break;
  }
}
if (AllNT)
  cast<Instruction>(MemCmpCall)->setMetadata(
      LLVMContext::MD_nontemporal,
      MDNode::get(Phi.getContext(),
                  ConstantAsMetadata::get(
                      ConstantInt::get(Type::getInt32Ty(Phi.getContext()), 1))));
```

(Note: `memcmp` is a libcall; whether the backend respects
`!nontemporal` on a libcall is a separate question — but
`ExpandMemCmpPass` synthesizes real loads, and those should carry the
hint forward.)

## Status

Confirmed via `opt -passes=mergeicmps -S`. Same root area as w655
(missing AAMDNodes propagation), separate fix because `!nontemporal`
needs union-not-intersection semantics and lives outside the AAMDNodes
struct.

Source references from `llvm/lib/Transforms/Scalar/MergeICmps.cpp` (main).
