# `combineMetadata` iterates over K's metadata only — `!tbaa` (and any other kind) present on J but not K is silently DROPPED

**Pass surface:** any caller of `combineMetadataForCSE`. Confirmed for `early-cse`.
**Source:** `llvm/lib/Transforms/Utils/Local.cpp` lines 2934-2941:
```cpp
static void combineMetadata(Instruction *K, const Instruction *J,
                            bool DoesKMove, bool AAOnly = false) {
  SmallVector<std::pair<unsigned, MDNode *>, 4> Metadata;
  K->getAllMetadataOtherThanDebugLoc(Metadata);     // <-- only K's kinds
  for (const auto &MD : Metadata) {
    unsigned Kind = MD.first;
    MDNode *JMD = J->getMetadata(Kind);
    MDNode *KMD = MD.second;
    ...
```
**Triple:** `x86_64-unknown-linux-gnu`
**Tool:** `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -S -passes=early-cse`.

## Root cause

`combineMetadata` walks `K->getAllMetadataOtherThanDebugLoc(Metadata)`. Only kinds present on `K` are visited. Kinds present on `J` but not on `K` never reach the switch.

For symmetric metadata kinds (e.g., `!tbaa`, `!alias.scope`, `!noalias`, `!noalias_addrspace`), the correct merge would be:
- If `K` has it and `J` does not: keep `K`'s (drop if `DoesKMove=true` for AA-conservative)
- If `K` does not have it and `J` does: NEW — needs to be considered

The current code is asymmetric in the second case: J-only kinds are silently lost. For `DoesKMove=false` (CSE, K kept in place), this drop is harmless for K itself, but the *deleted J* had asserted those kinds — any caller that subsequently performs further inferences using K's metadata is missing J's evidence.

For `DoesKMove=true` (sink/hoist), the lost J-only kinds are critical: K is now serving J's path too. K should adopt the conservative-intersection across BOTH original metadatas — but if K never had the kind, the intersection step is skipped, so K inherits no information about J's annotated invariant. For some kinds (e.g. `!tbaa`, where "no metadata" == "may alias anything") this is conservative-safe. For others (e.g. `!nontemporal`, `!nosanitize`, `!noundef` — see line 3025-3043), the silent drop is asymmetric with how the kind would have been merged the other way.

## Reproducer — TBAA on J-only

```llvm
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @cse(ptr %p) {
entry:
  %0 = load i32, ptr %p, align 4
  %1 = load i32, ptr %p, align 4, !tbaa !0
  %add = add i32 %0, %1
  ret i32 %add
}

!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2}
!2 = !{!"omnipotent char", !3}
!3 = !{!"Simple C/C++ TBAA"}
```

```
$ opt -S -passes=early-cse repro.ll
```

After:
```
entry:
  %0 = load i32, ptr %p, align 4
  %add = add i32 %0, %0
  ret i32 %add
}
```

The `!tbaa` from `%1` was the only TBAA annotation in the function. After EarlyCSE merges `%1` into `%0`, the kept load has NO `!tbaa`. The annotation is gone — no `!0 = ...` in the output module either.

## Why this matters downstream

In a real -O2 pipeline, the TBAA was supplied by the front-end on `%1` for legitimate AA reasoning (e.g., "this access is to a `union { int; float; }`'s int field"). If `%1` was the first load to get TBAA annotation (e.g., from a hot loop), the EarlyCSE pass collapses it into a flag-less dominator load. Subsequent BasicAA/TBAA-aware passes can no longer prove this load doesn't alias floating-point stores. This results in pessimistic LICM/DSE/MemCpyOpt decisions.

## Pipeline reproducer

The IR survives `-O2`:
```
$ opt -S -O2 repro.ll | grep -c 'tbaa'
0
```

## Fix sketch

In addition to iterating `K`'s metadata, also iterate metadata kinds present on `J` but not on `K`, and apply the same per-kind merge rule (treating `KMD = nullptr`). This is required for sound merging of AA-relevant kinds when `DoesKMove=true`, and for preserving J's assertions when `DoesKMove=false` (e.g. K could ADOPT J's invariant.load if J asserts it and the use-site is dominated by K).
