# w533: LowerExpect `getBranchWeight` divides by zero for `expect.with.probability` on switch with no cases

## Summary
`getBranchWeight` for `expect_with_probability` computes
`FalseProb = (1.0 - TrueProb) / (BranchCount - 1)`. When `BranchCount == 1`
(a `switch` with only its default label and zero case entries) this divides
by zero, producing `inf`/`nan` doubles, then casts `ceil(inf)` to
`uint32_t`. The C++ standard makes a float-to-unsigned conversion of an
out-of-range value (including `inf` and `nan`) undefined behavior.

## Source
File: `llvm/lib/Transforms/Scalar/LowerExpectIntrinsic.cpp`

```cpp
// lines 55-74
static std::tuple<uint32_t, uint32_t>
getBranchWeight(Intrinsic::ID IntrinsicID, CallInst *CI, int BranchCount) {
  if (IntrinsicID == Intrinsic::expect) { ... }
  else {
    auto *Confidence = cast<ConstantFP>(CI->getArgOperand(2));
    double TrueProb = Confidence->getValueAPF().convertToDouble();
    ...
    double FalseProb = (1.0 - TrueProb) / (BranchCount - 1);   // <-- div 0
    uint32_t LikelyBW = ceil((TrueProb * (double)(INT32_MAX - 1)) + 1.0);
    uint32_t UnlikelyBW = ceil((FalseProb * (double)(INT32_MAX - 1)) + 1.0);
    return std::make_tuple(LikelyBW, UnlikelyBW);
  }
}
```

Caller `handleSwitchExpect` passes `n + 1` where `n = SI.getNumCases()`
(line 92, 95). A switch is legal with zero cases (only `label %default`), in
which case `n+1 == 1` and the division divides by 0.

Either result is then **always** computed even if it will not be stored,
because `Weights(n+1, UnlikelyBranchWeightVal)` (line 97) initialises the
vector with `UnlikelyBW` before the loop overwrites the likely slot.

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @sw(i32 %x) {
entry:
  %e = call i32 @llvm.expect.with.probability.i32(i32 %x, i32 7, double 5.000000e-01)
  switch i32 %e, label %def []   ; zero cases
def:
  ret i32 2
}
declare i32 @llvm.expect.with.probability.i32(i32, i32, double)
```

Run:
```
opt -passes=lower-expect -S
```

## Observed output
```
  switch i32 %x, label %def [
  ], !prof !0
!0 = !{!"branch_weights", !"expected", i32 1073741824}
```

The numeric weight stored ends up being `LikelyBW = 1073741824`. The
problematic value (`UnlikelyBW` from `ceil(inf)` cast to `uint32_t`) is
computed and stored into the initialiser of `SmallVector<uint32_t> Weights`
before being overwritten - undefined behavior per C++ even though the value
is later replaced. UBSan / a future stricter compilation of LLVM with
`-fsanitize=float-cast-overflow` would flag this; today it works only by
chance because most implementations clamp `inf -> uint32_t` to
`UINT32_MAX`.

## Triggering paths
1. **Direct UB**: any zero-case switch behind `expect.with.probability`.
2. **Latent miscompile**: any branch/switch where the user explicitly sets
   `TrueProb == 1.0` and `BranchCount == 1`. The `FalseProb` arm still
   evaluates `0/0 = nan`, then `ceil(nan*X + 1) -> nan -> uint32` is UB.

A trivial fix: guard with `if (BranchCount <= 1) FalseProb = 0;` (or assert
that `BranchCount >= 2` and require the switch lowering caller to bail on
zero-case switches).

## Default-pipeline confirmation
Default `opt -passes=lower-expect`; the pass runs in default `-O2`.
