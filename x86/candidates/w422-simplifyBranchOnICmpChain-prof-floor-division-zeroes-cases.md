# w422 — `simplifyBranchOnICmpChain` distributes `!prof` over switch cases with floor division, zeroing all case weights when taken-weight < case count

Severity: !prof corruption / loss of profile signal. Not a miscompile of
program behavior, but corrupts PGO information that downstream passes (and the
inliner / block placement) consume.

## Where

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp:5307-5316`

```cpp
SwitchInst *New = Builder.CreateSwitch(CompVal, DefaultBB, Values.size());
if (HasProfile) {
  // We know the weight of the default case. We don't know the weight of the
  // other cases, but rather than completely lose profiling info, we split
  // the remaining probability equally over them.
  SmallVector<uint32_t> NewWeights(Values.size() + 1);
  NewWeights[0] = BranchWeights[1]; // this is the default, and we swapped
                                    // if TrueWhenEqual.
  for (auto &V : drop_begin(NewWeights))
    V = BranchWeights[0] / Values.size();    // <-- floor division
  setBranchWeights(*New, NewWeights, /*IsExpected=*/false);
}
```

## What's wrong

When the icmp-chain-to-switch fold splits the original "taken" branch weight
across N case values, it uses integer floor division
`BranchWeights[0] / Values.size()`. If `BranchWeights[0] < Values.size()` (a
small "taken" weight relative to the chain length), this yields **0** for
every case. The taken-branch profile signal is then completely lost — the new
switch's metadata claims all cases are unreachable, while the default has the
full weight.

This is documented as a deliberate simplification ("we split the remaining
probability equally over them") but the rounding choice is wrong: with N
cases sharing a taken weight of 3, you should either:

1. Distribute the remainder (e.g. `{1,1,1,0,0}` for `3/5`), or
2. Round up so a small-but-nonzero taken weight is preserved as 1 per case
   (`ceildiv(3,5) = 1`), or
3. At minimum, special-case `BranchWeights[0] > 0` and produce 1 instead of
   silently zeroing — otherwise PGO information that says "this branch is
   taken ~3% of the time" becomes "this branch is never taken".

The default case weight is left untouched (`NewWeights[0] = BranchWeights[1]`),
so the *ratio* default-vs-cases is corrupted asymmetrically.

## Reproducer

`/tmp/w420/t39_icmp_chain_zeros.ll`:

```ll
declare void @t()
declare void @f0()

define void @f(i32 %x) {
entry:
  %a = icmp eq i32 %x, 11
  %b = icmp eq i32 %x, 22
  %c = icmp eq i32 %x, 33
  %d = icmp eq i32 %x, 44
  %e = icmp eq i32 %x, 55
  %or1 = or i1 %a, %b
  %or2 = or i1 %or1, %c
  %or3 = or i1 %or2, %d
  %or4 = or i1 %or3, %e
  br i1 %or4, label %taken, label %fall, !prof !0
taken:
  call void @t()
  ret void
fall:
  call void @f0()
  ret void
}
!0 = !{!"branch_weights", i32 3, i32 100}
```

Pipeline confirmed default: `opt -passes=simplifycfg -S`. No non-default
SimplifyCFG option needed; `simplifyBranchOnICmpChain` is reached from
`simplifyCondBranch` at `SimplifyCFG.cpp:8585`.

After `opt -passes=simplifycfg -S`:

```ll
define void @f(i32 %x) {
entry:
  switch i32 %x, label %fall [
    i32 55, label %taken
    i32 44, label %taken
    i32 33, label %taken
    i32 22, label %taken
    i32 11, label %taken
  ], !prof !0
  ...
}

!0 = !{!"branch_weights", i32 100, i32 0, i32 0, i32 0, i32 0, i32 0}
```

Input: taken weight 3 (~2.9% probability), fall weight 100.
Output: default = 100, every case = `3 / 5 = 0`. Total weight on the 5
"taken" cases is 0, so the new metadata claims the entire 3% taken-branch
probability has vanished.

## Severity / class

PGO / branch-weight corruption. Concrete downstream impact:

- Block placement (`MachineBlockPlacement`) will now treat the cases as
  cold/unreachable.
- The inliner uses `!prof` to gate hot/cold inlining decisions — `0/0/0/0/0`
  here may push otherwise-hot callees onto a cold path.
- Any later `simplifycfg<switch-range-to-icmp>` (enabled in O2) will read
  these weights when transforming the switch back into a compare-and-branch,
  propagating the zeros downstream.

Not a miscompile of program behavior; it is a miscompile of profile data.

## Notes

- Suggested fix: use ceiling division when `BranchWeights[0] > 0`, or
  distribute the floor-division remainder cyclically across the first
  `BranchWeights[0] % N` cases. A one-line fix:
  ```cpp
  uint32_t share = std::max<uint32_t>(BranchWeights[0] / Values.size(),
                                      BranchWeights[0] ? 1u : 0u);
  ```
- The companion "range-to-icmp" path at line 5288-5302 uses the *raw*
  `BranchWeights` unchanged (`setBranchWeights(*NewBI, BranchWeights, ...)`),
  which is correct because it produces a CondBr with the same 2 successors.
  Only the multi-case path loses signal.
- Lines 5311-5316 also assume `Values.size() >= 1`; that is guaranteed by the
  `UsedICmps <= 1` early return at line 5203, so no divide-by-zero here.
