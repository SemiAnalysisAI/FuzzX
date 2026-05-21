# w468: IndirectCallPromotion `tryToPromoteWithFuncCmp` underflows `TotalCount` when individual VP counts saturate above their stored sum (release builds only)

File: `llvm/lib/Transforms/Instrumentation/IndirectCallPromotion.cpp`
Lines: 696-723 (focus 700-701)

## Summary

`tryToPromoteWithFuncCmp` walks the candidate list and after each
`promoteIndirectCall` it subtracts the promoted count from the running
`TotalCount`:

```cpp
for (const auto &C : Candidates) {
  uint64_t FuncCount = C.Count;
  pgo::promoteIndirectCall(CB, C.TargetFunction, FuncCount, TotalCount,
                           SamplePGO, &ORE);
  assert(TotalCount >= FuncCount);          // <-- only guards debug builds
  TotalCount -= FuncCount;                  // <-- unsigned underflow in release
  ...
}
```

The companion vtable-cmp path (lines 837-843) explicitly documents that
`TotalCount` is a **saturated sum** of individual VP counts, and that
individual counts can therefore exceed `TotalCount` after promotion:

```cpp
assert(TotalFuncCount >= Candidate.Count &&
       "Within one prof metadata, total count is the sum of counts from "
       "individual <target, count> pairs");
// Use std::min since 'TotalFuncCount' is the saturated sum of individual
// counts, see
// https://github.com/llvm/llvm-project/blob/.../llvm/lib/ProfileData/InstrProf.cpp#L1281-L1288
TotalFuncCount -= std::min(TotalFuncCount, Candidate.Count);
```

i.e. the func-cmp path on line 701 has the same hazard that the
vtable-cmp path on line 843 was patched to guard against, but never
received the same `std::min` fix. With assertions stripped (`-DNDEBUG`,
i.e. the production-quality build), feeding ICP a VP node whose
individual `Count` fields sum above `TotalCount` produces an unsigned
underflow at line 701. The wrapped, near-`UINT64_MAX` `TotalCount` is
then passed to:

* subsequent loop iterations (more wraps and bogus weights),
* `promoteIndirectCall` on the next candidate (the `TotalCount` argument
  drives the `branch_weights` created at line 671 via
  `createBranchWeights(... Count, TotalCount - Count)` — a wrapped
  `TotalCount` produces a `branch_weights` that gives the cold
  fallback a count ~ `UINT64_MAX - Count`, which scales every other
  branch-probability decision in the post-ICP module),
* `updateFuncValueProfiles` at line 731 (the residual `TotalCount` ends
  up in the rebuilt VP node's "Total" field).

This is the exact same pattern as the saturated-sum issue the vtable-cmp
path noted, just with the surviving asymmetric ASSERT-guarded branch.

## Citation

func-cmp path (buggy):
```cpp
// IndirectCallPromotion.cpp:696-702
for (const auto &C : Candidates) {
  uint64_t FuncCount = C.Count;
  pgo::promoteIndirectCall(CB, C.TargetFunction, FuncCount, TotalCount,
                           SamplePGO, &ORE);
  assert(TotalCount >= FuncCount);
  TotalCount -= FuncCount;
```

vtable-cmp path (saturating):
```cpp
// IndirectCallPromotion.cpp:837-843
assert(TotalFuncCount >= Candidate.Count &&
       "Within one prof metadata, total count is the sum of counts from "
       "individual <target, count> pairs");
// Use std::min since 'TotalFuncCount' is the saturated sum of individual
// counts, see
// https://github.com/llvm/llvm-project/blob/abedb3b8356d5d56f1c575c4f7682fba2cb19787/llvm/lib/ProfileData/InstrProf.cpp#L1281-L1288
TotalFuncCount -= std::min(TotalFuncCount, Candidate.Count);
```

## Why it's a bug pattern match

"!prof scaling overflow" — saturated VP `Count` fields drive an unsigned
underflow at line 701 that is then propagated to
`createBranchWeights(Count, TotalCount-Count)` for the next candidate,
yielding wildly miscalibrated branch weights on every promoted indirect
call following the wrap.
