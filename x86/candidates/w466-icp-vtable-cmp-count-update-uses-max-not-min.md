# w466: IndirectCallPromotion `tryToPromoteWithVTableCmp` updates ICallProfDataRef[].Count with `std::max` instead of `std::min` — saturates wrong on inequality

File: `llvm/lib/Transforms/Instrumentation/IndirectCallPromotion.cpp`
Lines: 856-860

## Summary

After promoting a list of indirect-call candidates via vtable comparison,
the pass loops over `PromotedFuncCount` and tries to subtract the promoted
count from the corresponding entry of the value-profile array
`ICallProfDataRef`:

```cpp
for (size_t I = 0; I < PromotedFuncCount.size(); I++) {
  uint32_t Index = PromotedFuncCount[I].first;
  ICallProfDataRef[Index].Count -=
      std::max(PromotedFuncCount[I].second, ICallProfDataRef[Index].Count);
}
```

The arithmetic uses `std::max`, which is upside-down. The intent (compare
with the parallel `tryToPromoteWithFuncCmp` path on line 706, which simply
does `ICallProfDataRef[C.Index].Count = 0;`, and with the explicit
saturation comment at lines 840-843) is clearly to subtract the
*smaller* of the two values so we never underflow:

* `Promoted == Current` (the common case in well-formed IR):
  `max = Promoted = Current`, `Count -= Count = 0` — happens to be correct.
* `Promoted < Current` (e.g. an out-of-tree pass or LTO summary merge
  modified `Current` upward): `max = Current`, `Count -= Current = 0` — the
  remaining `Current - Promoted` survivors are silently dropped.
* `Promoted > Current` (the saturated-sum case the comment at line 841
  warns about, where individual counts saturate above their stored sum):
  `max = Promoted > Current`, `Count -= Promoted` **wraps around** to
  ~`UINT64_MAX`, which then gets written back into the regenerated VP
  metadata by `updateFuncValueProfiles`.

The same `Candidate.Count` is reused both as `Promoted` (kept in
`PromotedFuncCount.second`) and as the source of the original `Count` in
the metadata, so in mainline-built input the two are equal and the bug is
latent. But any IR with mutated VP counts (e.g. PGOICall run after
profile-summary scaling, or a fuzz-generated VP with deliberately
inconsistent `Candidate.Count`/`ICallProfDataRef[].Count`) triggers
either silent count loss or a wrap-around to ~`UINT64_MAX` that the
subsequent `updateFuncValueProfiles` will then write into the regenerated
!prof.

The companion func-cmp path on line 706 uses the obviously correct
`= 0`. The vtable-cmp path should mirror it (or at minimum use
`std::min`).

## Citation

```cpp
// IndirectCallPromotion.cpp:855-862
// FIXME: When Clang `-fstrict-vtable-pointers` is enabled, ...
for (size_t I = 0; I < PromotedFuncCount.size(); I++) {
  uint32_t Index = PromotedFuncCount[I].first;
  ICallProfDataRef[Index].Count -=
      std::max(PromotedFuncCount[I].second, ICallProfDataRef[Index].Count);
}
updateFuncValueProfiles(CB, ICallProfDataRef, TotalFuncCount, NumCandidates);
```

vs. the symmetric func-cmp path on line 706:
```cpp
// Update the count and this entry will be erased later.
ICallProfDataRef[C.Index].Count = 0;
```

vs. the explicit saturating note for `TotalFuncCount` at lines 841-843:
```cpp
// Use std::min since 'TotalFuncCount' is the saturated sum of individual
// counts, see ...
TotalFuncCount -= std::min(TotalFuncCount, Candidate.Count);
```

## Why it's a bug pattern match

"!prof scaling overflow" — when `Promoted > Current`, the unsigned
subtraction wraps to ~`UINT64_MAX`, which is then materialized back
into the rebuilt !prof MDNode by `updateFuncValueProfiles`.
