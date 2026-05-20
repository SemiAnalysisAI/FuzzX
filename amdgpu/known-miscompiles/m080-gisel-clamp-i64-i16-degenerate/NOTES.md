# m080: GlobalISel `matchClampI64ToI16` accepts degenerate clamp patterns and rewrites them to a real `med3` clamp

*Discovery method: code inspection.* Found by reading the AMDGPU
GlobalISel pre-legalizer combiner
`AMDGPUPreLegalizerCombiner.cpp::matchClampI64ToI16`.  The validator
checks only that both compare constants fit in an `i16`, but does not
check that the ordering of those constants is consistent with the
matched min/max pattern.  When the inequality is reversed the IR is
actually a constant expression, but `applyClampI64ToI16` rewrites it
into `v_med3_i32` of the same two constants — a real clamp that returns
the input value when it falls inside the range.

## Manual reproducer

The bug only triggers with GlobalISel.  The standard
`run_ll_reproducer.sh` script does not pass `-mllvm -global-isel`, so it
will report `mismatch=false`.  To reproduce by hand:

```bash
cd /tmp/findbug/med3
clang -O0 -nogpulib -target amdgcn-amd-amdhsa -mcpu=gfx950 \
    -mllvm -global-isel -x ir -c reduced.ll -o reduced.O0.gisel.o
lld -flavor gnu -shared reduced.O0.gisel.o -o reduced.O0.gisel.hsaco
# input lo=50, hi=0 -> origin=50
printf '\x32\x00\x00\x00\x00\x00\x00\x00' > input.bin
hip_module_runner reduced.O0.gisel.hsaco input.bin output.bin 2 0 2
xxd output.bin
# 00000000: 3200 0000 3200 0000  -- decimal 50, 50  (WRONG, should be 5)
```

## Root cause

`llvm/lib/Target/AMDGPU/AMDGPUPreLegalizerCombiner.cpp` lines ~131-167:

```cpp
auto IsApplicableForCombine = [&MatchInfo]() -> bool {
  const auto Cmp1 = MatchInfo.Cmp1;
  const auto Cmp2 = MatchInfo.Cmp2;
  const auto Diff = std::abs(Cmp2 - Cmp1);
  if (Diff == 0 || Diff == 1)
    return false;
  const int64_t Min = std::numeric_limits<int16_t>::min();
  const int64_t Max = std::numeric_limits<int16_t>::max();
  // Check if the comparison values are between SHORT_MIN and SHORT_MAX.
  return ((Cmp2 >= Cmp1 && Cmp1 >= Min && Cmp2 <= Max) ||      // (A)
          (Cmp1 >= Cmp2 && Cmp1 <= Max && Cmp2 >= Min));       // (B)
};

// Pattern 1: trunc i16 (smin(smax(Origin, Cmp2), Cmp1))
if (mi_match(MI.getOperand(1).getReg(), MRI,
             m_GSMin(m_Reg(Base), m_ICst(MatchInfo.Cmp1)))) {
  if (mi_match(Base, MRI,
               m_GSMax(m_Reg(MatchInfo.Origin), m_ICst(MatchInfo.Cmp2)))) {
    return IsApplicableForCombine();
  }
}

// Pattern 2: trunc i16 (smax(smin(Origin, Cmp2), Cmp1))
if (mi_match(MI.getOperand(1).getReg(), MRI,
             m_GSMax(m_Reg(Base), m_ICst(MatchInfo.Cmp1)))) {
  if (mi_match(Base, MRI,
               m_GSMin(m_Reg(MatchInfo.Origin), m_ICst(MatchInfo.Cmp2)))) {
    return IsApplicableForCombine();
  }
}
```

`IsApplicableForCombine` is `(A) || (B)`, so it accepts whenever *either*
ordering of `Cmp1` and `Cmp2` would produce a valid i16 clamp.  The
matcher does not tell the validator which of pattern 1 or pattern 2
fired.

* Pattern 1 (`smin(smax(X, Cmp2), Cmp1)`) is a real clamp to
  `[Cmp2, Cmp1]` only when `Cmp2 <= Cmp1` (clause **(B)** in the
  validator).  When `Cmp2 > Cmp1` the expression is identically `Cmp1`
  because `smax(X, Cmp2) >= Cmp2 > Cmp1`, so `smin(..., Cmp1) = Cmp1`.
* Pattern 2 (`smax(smin(X, Cmp2), Cmp1)`) is a real clamp to
  `[Cmp1, Cmp2]` only when `Cmp1 <= Cmp2` (clause **(A)**).

`applyClampI64ToI16` then unconditionally builds
`med3(min(Cmp1, Cmp2), packed_X, max(Cmp1, Cmp2))`, which is a real
clamp regardless of which pattern was matched.  For the degenerate case
the IR semantic (always `Cmp1`) is replaced by `clamp(X, lo, hi)`, so
any `X` that falls inside the clamp range surfaces as the result.

Concretely: with `Cmp1 = 5`, `Cmp2 = 100`, pattern 1, and `Origin = 50`,
the IR semantic is `5` but the apply produces `med3(5, 50, 100) = 50`.

## Fix sketch

Make the validator pattern-aware.  Simplest fix: drop the union-of-both
clauses and require the natural ordering for each pattern.

```cpp
// Returns true iff [lo,hi] = [min(Cmp1,Cmp2), max(Cmp1,Cmp2)] is a real
// i16 clamp range AND the matched pattern produces that clamp.
auto IsApplicableForCombine = [&MatchInfo](bool ExpectCmp1Hi) -> bool {
  const auto Cmp1 = MatchInfo.Cmp1, Cmp2 = MatchInfo.Cmp2;
  const auto Lo = std::min(Cmp1, Cmp2), Hi = std::max(Cmp1, Cmp2);
  if (Hi - Lo <= 1)
    return false;
  if (Lo < std::numeric_limits<int16_t>::min() ||
      Hi > std::numeric_limits<int16_t>::max())
    return false;
  // Pattern 1 (smin outer) needs Cmp1 to be the high bound.
  // Pattern 2 (smax outer) needs Cmp1 to be the low bound.
  return ExpectCmp1Hi ? (Cmp1 >= Cmp2) : (Cmp1 <= Cmp2);
};
// then: IsApplicableForCombine(/*Cmp1Hi=*/true) in the smin-outer arm,
//       IsApplicableForCombine(/*Cmp1Hi=*/false) in the smax-outer arm.
```

## Why the fuzzer doesn't see it

* The clang driver does not pass `-fglobal-isel` for AMDGPU compute by
  default, and the FuzzX `run_ll_reproducer.sh` harness invokes clang
  with only `-O0` and `-O2` (no `-mllvm -global-isel`).  The AMDGPU
  SelectionDAG path has no equivalent of `matchClampI64ToI16`, so the
  default toolchain does not exercise the buggy code at all.
* If the fuzzer ever did enable GlobalISel, at `-O1`+ InstCombine would
  see that `smin(smax(X, Cmp2), Cmp1)` with `Cmp1 < Cmp2` is identically
  `Cmp1` and constant-fold it before IRTranslation, removing the
  pattern.  Only the combination of (GlobalISel enabled) + (no
  InstCombine) + (degenerate constant ordering) survives long enough to
  reach the buggy combine.  The pre-legalizer combiner itself does not
  gate this rule on optimization level, so once we get there the rule
  fires.

## Files

* `/home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUPreLegalizerCombiner.cpp` — `matchClampI64ToI16` and `applyClampI64ToI16` (lines ~115-207).
