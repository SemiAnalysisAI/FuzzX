# EarlyCSE call CSE doesn't call combineMetadataForCSE (metadata on the removed call is silently dropped)

## Location
`llvm/lib/Transforms/Scalar/EarlyCSE.cpp` — call CSE block at lines 1651-1685, specifically line 1666. Compare to the *load* CSE block at lines 1607-1626 which DOES call `combineMetadataForCSE`:

```cpp
// EarlyCSE.cpp:1615 (load CSE)
if (InVal.IsLoad)
    if (auto *I = dyn_cast<Instruction>(Op))
        combineMetadataForCSE(I, &Inst, false);

// EarlyCSE.cpp:1666 (call CSE)
combineIRFlags(Inst, InVal.first);
// no combineMetadataForCSE call
```

## Root cause
The call CSE block only intersects IR flags via `combineIRFlags`. It does not merge metadata. Any `!range`, `!nonnull`, `!nofpclass`, `!noundef`, `!noalias`, `!alias.scope`, etc. that the *removed* call carried is just dropped: only the kept call's metadata survives. For `!range` in particular, this is asymmetric to what `combineMetadataForCSE` would have done at `DoesKMove=false` (where `!range` is merged via `getMostGenericRange` provided neither call has `!noundef`).

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"

declare i32 @ext_readonly() readnone

define {i32, i32} @f() {
  %v1 = call i32 @ext_readonly(), !range !0
  %v2 = call i32 @ext_readonly(), !range !1
  %r1 = insertvalue {i32, i32} undef, i32 %v1, 0
  %r2 = insertvalue {i32, i32} %r1, i32 %v2, 1
  ret {i32, i32} %r2
}

!0 = !{i32 0, i32 100}
!1 = !{i32 50, i32 60}
```

## opt diff
Before `opt -passes='early-cse<memssa>' -S`:
```
%v1 = call i32 @ext_readonly(), !range !0   ; range [0,100)
%v2 = call i32 @ext_readonly(), !range !1   ; range [50,60)
```

After:
```
%v1 = call i32 @ext_readonly(), !range !0   ; only [0,100)
; %v2 is removed; uses get %v1 with the wider range
```

The `[50,60)` range that `%v2` carried is gone — neither preserved on the kept call nor merged with it.

## Why it is wrong
Because EarlyCSE establishes that both calls return the same value, BOTH range assertions are simultaneously true at runtime: the value is in `[0,100) ∩ [50,60) = [50,60)`. A correct merge would replace the kept call's range with the *intersection*, i.e. `[50,60)`. The current code just keeps whatever the first call had (`[0,100)`), which is the *less* informative range. This is a missed optimization at best; if a downstream pass used the dropped `[50,60)` to prove a property (e.g., the high bits of the result are zero, enabling a smaller-width version of the consumer), the optimization no longer fires.

The same is true for `!noundef` (one call asserting noundef, the other not — the noundef should be preserved if BOTH had it; but here it's whatever the kept call has) and `!nonnull`. The `combineMetadataForCSE` helper handles all of these cases for the load path.

This is fundamentally different from the load case: for loads, EarlyCSE goes through `combineMetadataForCSE` which is the helper used by load CSE elsewhere in LLVM. For calls, that step is just missing.

## Suggested fix
Mirror the load-CSE path by calling `combineMetadataForCSE(InVal.first, &Inst, /*DoesKMove=*/false)` right after `combineIRFlags(Inst, InVal.first)` at line 1666. This will at minimum intersect/union the relevant kinds correctly per the existing helper logic.

## Status
REPRODUCIBLE at IR level. The merged range information is `[0,100)` even though both calls were proved to return the same value, so the program-wide-true range is `[50,60)`. The result is a missed optimization (technically not a miscompile, but a real correctness-of-metadata defect: post-CSE the IR claims weaker properties than the pre-CSE IR did, even though the pass is supposed to be value-preserving).
