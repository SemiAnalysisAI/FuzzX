# w286: GVN load CSE widens factually narrower `!range` on surviving loads

**Severity:** Missed optimization / lost facts. Soft soundness adjacent — if
the user encoded the narrower range as a known fact (e.g. from a previous
range check) the optimizer forgets it.

**Where:**
- `llvm/lib/Transforms/Scalar/GVN.cpp:1095-1136` (`ConstructSSAForLoadSet`)
- `llvm/lib/Transforms/Scalar/GVN.cpp:1144-1158` (`MaterializeAdjustedValue`)
- `llvm/lib/Transforms/Utils/Local.cpp:2972-2975` (`combineMetadata` `MD_range`)

(file paths under `/home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/`)

## Root cause

When GVN eliminates a non-local load via `processNonLocalLoad`, it builds
the SSA replacement through `ConstructSSAForLoadSet`. For each available
value in a predecessor block it invokes:

```cpp
1132:    SSAUpdate.AddAvailableValue(BB, AV.MaterializeAdjustedValue(Load));
```

For the `isCoercedLoadValue` same-type same-offset case,
`MaterializeAdjustedValue` runs

```cpp
1155:    LoadInst *CoercedLoad = getCoercedLoadValue();
1156:    if (CoercedLoad->getType() == LoadTy && Offset == 0) {
1157:      Res = CoercedLoad;
1158:      combineMetadataForCSE(CoercedLoad, Load, false);
1159:    }
```

`combineMetadataForCSE(K=CoercedLoad, J=Load, DoesKMove=false)` runs the
`MD_range` arm of `combineMetadata` in `Local.cpp:2972`:

```cpp
2972:      case LLVMContext::MD_range:
2973:        if (!AAOnly && (DoesKMove || !K->hasMetadata(LLVMContext::MD_noundef)))
2974:          K->setMetadata(Kind, MDNode::getMostGenericRange(JMD, KMD));
2975:        break;
```

This *widens* `CoercedLoad`'s `!range` to the union with the eliminated
`Load`'s `!range`. The result: the surviving load (the one the user wrote
in the predecessor) silently loses its narrower, factual `!range` and
ends up with whatever broader range the eliminated load had.

Symmetrically `MD_nofpclass` (line 2976-2978) and `MD_align` (line 3011-3014)
are widened with the same `(!K->hasMetadata(noundef))` predicate.

`MaterializeAdjustedValue` is called *once per available block* during phi
construction (line 1132), so every surviving load in every predecessor gets
widened — even though the eliminated `Load` is just one site in the join.

## Reproducer

```ll
; opt -passes=gvn -S
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @use(i32) memory(none)

define i32 @f(i1 %c, ptr %p) {
entry:
  br i1 %c, label %a, label %b

a:
  %la = load i32, ptr %p, align 4, !range !0       ; [0,3)  user-known fact
  call void @use(i32 %la)
  br label %merge

b:
  %lb = load i32, ptr %p, align 4, !range !1       ; [3,5)  user-known fact
  call void @use(i32 %lb)
  br label %merge

merge:
  %v = load i32, ptr %p, align 4, !range !2        ; [0,5)  conservative
  ret i32 %v
}

!0 = !{i32 0, i32 3}
!1 = !{i32 3, i32 5}
!2 = !{i32 0, i32 5}
```

Before GVN: `%la` is constrained to `[0,3)`, `%lb` to `[3,5)`.

`opt -passes=gvn -S` produces:

```ll
a:
  %la = load i32, ptr %p, align 4, !range !0       ; <-- now [0,5)
  call void @use(i32 %la)
  br label %merge

b:
  %lb = load i32, ptr %p, align 4, !range !0       ; <-- now [0,5)
  call void @use(i32 %lb)
  br label %merge

merge:
  %v = phi i32 [ %lb, %b ], [ %la, %a ]
  ret i32 %v

!0 = !{i32 0, i32 5}
```

`%la`'s `!range` widened from `[0,3)` to `[0,5)`, `%lb`'s from `[3,5)` to
`[0,5)`. The narrower constraints — true facts about those particular load
sites — are gone. The phi merge `%v` has no `!range` (PHIs cannot).

Downstream: a `switch i32 %la, [...]` that previously knew it only had to
handle values 0..2 must now handle 0..4. A `shl i32 %la, 30` that was
provably `nuw nsw` against `[0,3)` is no longer provable.

## Why this is unsafe-ish

`combineMetadataForCSE` is designed for cases where K and J are
intersected at K's site. With `DoesKMove=false`, the contract is "K stays
put, J was at a different site". For `MD_range`, the intersection /
generic-range widening *is* sound (the loaded value still must satisfy the
union range at K's site), but it conflates two different program points
into one assertion.

The pre-existing TBAA arm (`Local.cpp:2954-2957`) only intersects when
`DoesKMove`. The range arm dropping `DoesKMove` guard for the no-noundef
case is suspicious — for CSE we know the predecessor load's `!range` was
true at the predecessor site, regardless of what the join load says.
Widening the predecessor's range with the join's range only adds noise.

## Suggested fix

Tighten the `MD_range` (and `MD_nofpclass`, `MD_align`) cases:

```cpp
case LLVMContext::MD_range:
  if (!AAOnly && DoesKMove)        // only widen when K moved
    K->setMetadata(Kind, MDNode::getMostGenericRange(JMD, KMD));
  else if (!AAOnly && !K->hasMetadata(LLVMContext::MD_noundef))
    // K stayed put; only widen if K had no range or J's range is broader
    K->setMetadata(Kind, MDNode::getMostGenericRange(JMD, KMD));
  // else: K's existing narrower !range is correct, leave it
  break;
```

The simplest correct rule for CSE-without-move is: keep K's range as-is
(it's a true fact at K's site); don't widen.

## Default x86 -O2 only

Reproduces with `opt -passes=gvn -S` on `x86_64-unknown-linux-gnu`; no
source-level changes required.
