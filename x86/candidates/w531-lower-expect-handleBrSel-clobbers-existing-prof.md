# w531: LowerExpect handleBrSelExpect unconditionally clobbers existing `!prof`

## Summary
`handleBrSelExpect` always overwrites the `!prof` on the branch or select it
is updating, with no check for pre-existing metadata. If the user (or an
earlier pass) attached real profile weights to the `br i1` or `select`, they
are replaced by the synthetic `LikelyBranchWeight:UnlikelyBranchWeight`
(default 2000:1) of the expect lowering.

## Source
File: `llvm/lib/Transforms/Scalar/LowerExpectIntrinsic.cpp`

```cpp
// line 348
BSI.setMetadata(LLVMContext::MD_prof, Node);
```

Earlier the code builds `Node` from the expect intrinsic's weights (lines
332-339). There is no `if (hasBranchWeightMD(BSI)) return;` guard, and no
attempt to merge with any existing profile.

Unlike the phi-def path (w530) where the clobbered branch is in a
predecessor, here the branch *is* the one the expect informs - so an
overwrite is at least defensible if no profile existed. But silently
discarding measured PGO weights on the same branch is still wrong: a
frontend `__builtin_expect` is a *hint*, while PGO is *measured*, and the
documented convention (and what
`PGOInstrumentation::shouldKeepBranchWeights` upholds) is that measured
data wins.

## Reproducer
```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(i32 %x) {
entry:
  %e = call i32 @llvm.expect.i32(i32 %x, i32 1)
  %c = icmp eq i32 %e, 0
  br i1 %c, label %t, label %f, !prof !100   ; <-- real PGO data here
t:
  ret i32 1
f:
  ret i32 2
}
declare i32 @llvm.expect.i32(i32, i32)
!100 = !{!"branch_weights", i32 5000, i32 100}
```

## Observed diff
Before:
```
  br i1 %c, label %t, label %f, !prof !100
...
!100 = !{!"branch_weights", i32 5000, i32 100}
```
After (`opt -passes=lower-expect -S`):
```
  br i1 %c, label %t, label %f, !prof !0
...
!0 = !{!"branch_weights", !"expected", i32 1, i32 2000}
```

5000:100 became 1:2000 - not only is the original PGO gone, but the
direction was *flipped* (because the expect lowering puts the unlikely
weight on the now-`label %t` side per the `==` rule at line 330-339).

The same clobber also happens for `select` (the template is
`handleBrSelExpect<SelectInst>`).

## Impact
Any pipeline that runs `PGOInstrumentation`/`SampleProfileLoader` *before*
`LowerExpectIntrinsic` and on code containing `__builtin_expect` loses the
measured weights. In LLVM's default `-O2`, the expect lowering runs in the
function simplification pipeline, so any branch with attached PGO weights
that also has an unstripped `llvm.expect` user is at risk.

The `!"expected"` marker added by `MDBuilder::createBranchWeights` partially
mitigates this for *downstream* passes that respect `IsExpected`, but the
real numeric weights are already lost by that point.

## Default-pipeline confirmation
Default `opt -passes=lower-expect`; the pass is part of `-O2`.
