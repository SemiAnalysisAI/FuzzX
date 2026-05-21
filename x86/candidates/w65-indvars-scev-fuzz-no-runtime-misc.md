# worker-65: SCEV/IndVarSimplify fuzz session — no runtime miscompile confirmed

Files visited: `llvm/lib/Transforms/Scalar/IndVarSimplify.cpp`,
`llvm/lib/Analysis/ScalarEvolution.cpp`,
`llvm/lib/Transforms/Utils/SimplifyIndVar.cpp`,
`llvm/lib/Transforms/Scalar/LoopStrengthReduce.cpp`.

## Patterns exercised end-to-end (opt -passes=indvars or -O3, then llc -O2, vs
## llc -O0 of the same IR, with a C runner)

All matched between optimized and unoptimized (i.e., no observable runtime
miscompile in the constructed IR):

1. **Two-IV closing-rate (bug 088 / w44 shape)**: i32 IV `add nuw 1` + i32 RHS
   `add nsw -1`, latch `iv.next ult rhs.next`, `mustprogress` loop. Closed-form
   from SCEV verified against direct iteration for `rhs_start ∈ {1,2,3,5,10,
   100,1000}`. The (A)+(B) corner described in candidate w44 requires
   `rhs_start` small enough that RHS unsigned-underflows; in those inputs the
   loop is infinite at i32 wrap *or* hits NSW signed overflow → UB, so
   downstream wrong answer cannot be observed without UB.

2. **Negative-stride loop**: `iv = phi i32 [n], iv.next = add nsw -1`,
   `cond = icmp sgt iv.next, 0`. SCEV produces closed-form `n*(n-1)/2`; matches
   reference for n up to 65536.

3. **`eliminateTrunc` path**: i64 IV widened then truncated, `icmp ult i32
   trunc(iv.next), trunc(n)`. SCEV builds `umax + half(n*(n-1))` formula using
   only the low 32 bits of `n`; verified for `n ∈ {1,…,1000, 0x100000000,
   0x100000001, 0x1FFFFFFFF, 0x200000005}`.

4. **LFTR with non-unit stride**: stride 3, exit `icmp slt iv.next, n`. SCEV
   produces sum of arithmetic progression with udiv-by-3 internal; matches for
   `n` up to 10^6.

5. **Two-IV multiplicative body**: `i*j` accumulator with `i++/j--`. Unrolled
   to closed form by `-O3`; matches for `n` up to 10^5.

6. **Geometric `shl nuw nsw i32, 1` loop**: SCEV does not model as AddRec;
   `indvars` does not transform; no regression.

7. **Post-inc with `ne` exit**: `iv.next = add nsw 1`, latch `icmp ne iv.next,
   n`. SCEV produces `n - s` directly. Matches for valid inputs (cases that
   would require IV wrap are NSW-UB).

8. **Trunc-based exit (`trunc i64 → i32 ne 0`)**: SCEV produces literal
   `12884901888 = 3 * 2^32` for `iv.next = add i64 3`, latch
   `(i32)iv.next != 0`. Mathematically correct (lcm of 3 and 2^32 stride).

## False alarms

* Initially flagged a "miscompile" in pattern 2/3 because the C reference
  returned `sum.next` while the IR returned `%sum` (phi value at start of last
  iter). After fixing the reference to read the phi value, results matched.

## What was not exercised

* `LoopStrengthReduce.cpp` runtime — only source-read.
* `simplifyAndExtend` / `WidenIV::widenLoopCompare` `samesign` predicate
  interactions (line 1629 of SimplifyIndVar.cpp). The `Cmp->hasSameSign() ?
  IsSigned : Cmp->isSigned()` selection is a recent change worth a targeted
  fuzzer pass.
* Multiple-exit loops with SCEV picking the wrong exiting block.
* SCEV `applyLoopGuards` and assume-bundle interactions.

## Conclusion

No new runtime miscompile from worker-65 in this hunt. Candidate w44
(equivalent to confirmed bug 088) remains the strongest unfiled suspicion in
this area; reproducing its (A)+(B)+(C) corner requires an IR shape that does
not have UB before the wrong-BECount window opens — likely needs a multi-exit
loop or a guard that bounds iv but not rhs.
