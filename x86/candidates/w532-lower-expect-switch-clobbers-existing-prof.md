# w532: LowerExpect handleSwitchExpect unconditionally clobbers existing switch `!prof`

## Summary
`handleSwitchExpect` calls `setBranchWeights(SI, Weights, /*IsExpected=*/true)`
on the switch unconditionally. Any pre-existing `!prof` on the switch (PGO
data or value-profile metadata in a related form) is replaced with the
synthetic 2000:1 expect weights with no merge and no opt-out.

## Source
File: `llvm/lib/Transforms/Scalar/LowerExpectIntrinsic.cpp`

```cpp
// lines 104-106
SI.setCondition(ArgValue);
setBranchWeights(SI, Weights, /*IsExpected=*/true);
return true;
```

`setBranchWeights` (in `IR/ProfDataUtils.cpp:325`) calls
`I.setMetadata(LLVMContext::MD_prof, BranchWeights)` - an unconditional
replace.

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @sw(i32 %x) {
entry:
  %e = call i32 @llvm.expect.i32(i32 %x, i32 1)
  switch i32 %e, label %def [
    i32 1, label %a
    i32 2, label %b
  ], !prof !100        ; <-- real PGO data
a:
  ret i32 1
b:
  ret i32 2
def:
  ret i32 0
}
declare i32 @llvm.expect.i32(i32, i32)
!100 = !{!"branch_weights", i32 10, i32 500, i32 400}
```

## Observed diff
Before:
```
  switch i32 %e, label %def [
    i32 1, label %a
    i32 2, label %b
  ], !prof !100
!100 = !{!"branch_weights", i32 10, i32 500, i32 400}
```
After (`opt -passes=lower-expect -S`):
```
  switch i32 %x, label %def [
    i32 1, label %a
    i32 2, label %b
  ], !prof !0
!0 = !{!"branch_weights", !"expected", i32 1, i32 2000, i32 1}
```

The measured switch profile `10:500:400` is replaced with the canonical
`1:2000:1` purely because someone tagged case 1 as `__builtin_expect`. Note
that the original PGO already correctly identified case 1 as the dominant
one (500 of ~910 total) - but the magnitudes (used by
`MachineBlockPlacement` and `BlockFrequencyInfo`) are now garbage.

## Impact
Same as w530/w531 but for switches. Particularly damaging because
multi-target switches drive jump-table-vs-if-cascade decisions, and the
case-by-case probabilities are exactly what `SwitchLowering` consumes.

## Default-pipeline confirmation
Default `opt -passes=lower-expect`; the pass runs in default `-O2`.
