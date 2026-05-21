# w262: JumpThreading PRE'd load passes `DoesKMove=true` for K that does **not** move, stripping K's `!range`/`!nontemporal`/`!nonnull`/`!invariant.load`

## Pass
`-passes=jump-threading` (default x86 -O2 pipeline includes this).

## Summary

`simplifyPartiallyRedundantLoad` is the PRE-style transform inside JT that
hoists a load from BB into its predecessors when the value is already
available on some incoming edge. For each predecessor where the load is
available (`PredLoadI`, "K"), it merges the metadata of the original load
(`LoadI`, "J") into the predecessor's load by calling:

```cpp
// JumpThreading.cpp:1459
for (LoadInst *PredLoadI : CSELoads) {
  combineMetadataForCSE(PredLoadI, LoadI, true);   // <-- DoesKMove = true
  LVI->forgetValue(PredLoadI);
}
```

`DoesKMove=true` tells `combineMetadata` (Local.cpp:2934) that K has been
relocated, so K can no longer rely on its source-context-specific
metadata. Concretely this means metadata like `!range`, `!nontemporal`,
`!nonnull`, `!invariant.load`, `!align`, `!dereferenceable[_or_null]`,
`!noundef`, `!noalias_addrspace` are **intersected** with J's, and if J
has no such metadata the result is `nullptr` — i.e. **K loses the
metadata even though K never moved**.

But `PredLoadI` is **not** moved by PRE. It stays at its original IR
location in the predecessor block. Its metadata reflects the load
operation at that location and is correct for all of K's existing
users. Marking it as "moved" causes JT to needlessly strip metadata
that was correctly applicable.

## Sites

- `JumpThreading.cpp:1459` — PRE'd load combine:
  ```cpp
  combineMetadataForCSE(PredLoadI, LoadI, true);
  ```
  Should be `/*DoesKMove=*/false` — the available pred load is not moved,
  it's just being given an additional set of users (via the inserted PHI
  in `LoadBB`).
- `Local.cpp:2972` (`MD_range`): intersected only when `DoesKMove`,
  falls back to nullptr when J has no range.
- `Local.cpp:2984` (`MD_invariant_load`): copy only when `DoesKMove`.
- `Local.cpp:3018` (`MD_dereferenceable*`): intersected when `DoesKMove`.
- `Local.cpp:3025` (`MD_noundef`): kept only when `DoesKMove` and present
  on both.

## Reproducer (range)

Input `final_c.ll`:
```llvm
target triple = "x86_64-unknown-linux-gnu"
declare i32 @f1()

define void @test_pre_load_range(ptr %p, ptr %dst, i1 %c) {
entry:
  br i1 %c, label %d1, label %d2
d1:
  %a = load i32, ptr %p, !range !0     ; <-- !range on K
  br label %d3
d2:
  %xxxx = tail call i32 @f1()
  br label %d3
d3:
  %x = phi i32 [ 1, %d2 ], [ %a, %d1 ]
  %b = load i32, ptr %p                ; <-- J (no !range)
  store i32 %x, ptr %dst
  %c2 = icmp eq i32 %b, 8
  br i1 %c2, label %ret1, label %ret2
ret1:
  ret void
ret2:
  %xxx = tail call i32 @f1()
  ret void
}
!0 = !{i32 10, i32 20}
```

```
opt -passes=jump-threading -S final_c.ll
```

Output (excerpt):
```llvm
d1:
  %a = load i32, ptr %p, align 4       ; <-- !range !0 has been stripped
  br label %d3

d2:
  %xxxx = tail call i32 @f1()
  %b.pr = load i32, ptr %p, align 4
  br label %d3
```

`%a` previously had `!range !0`, which is a fact about the *load
operation* at d1 and was valid for `%a`'s only original user (the PHI).
After PRE-style threading, `%a` is given an additional user (the
inserted `%b` PHI), but JT also **strips `%a`'s `!range`**. The range
fact was independent of who consumes the value — losing it is a
straight regression in downstream optimization.

Reproducible the same way for `!nontemporal`, `!nonnull`,
`!invariant.load`:

```llvm
; just replace the `!range !0` with !nontemporal/!nonnull/!invariant.load
```
in all three cases the metadata is stripped from `%a` after the pass.

## Why this matters

- `!range` informs LVI/ValueTracking and downstream codegen
  (`computeKnownBits`).
- `!nonnull`, `!align`, `!dereferenceable` enable many memory and
  pointer-safety optimizations.
- `!invariant.load` enables hoisting and CSE across calls/stores.
- `!nontemporal` selects non-temporal store/load codegen on x86; losing
  it on the pred load defeats the user's explicit hint.

Stripping any of these is a missed-opt at best and (for
`!invariant.load`) a quality-of-codegen drop the user explicitly
requested.

## Suggested fix

```cpp
// JumpThreading.cpp:1459
combineMetadataForCSE(PredLoadI, LoadI, /*DoesKMove=*/false);
```
The pred load is not moving, so `DoesKMove` must be false. This is
symmetric with the local-CSE call at JumpThreading.cpp:1259 which
correctly passes `false`.

## Local CSE call (already correct, for context)

```cpp
// JumpThreading.cpp:1259
combineMetadataForCSE(NLoadI, LoadI, false);
```
This is the within-block CSE; NLoadI stays put. The PRE call should
match.
