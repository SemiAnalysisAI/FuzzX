# 016 — X86InsertVZeroUpper `clobbersAllYmmAndZmmRegs` ignores YMM16-31 / ZMM16-31

Component: X86InsertVZeroUpper

## Source

`llvm/lib/Target/X86/X86InsertVZeroUpper.cpp:135-159`

```cpp
static bool clobbersAllYmmAndZmmRegs(const MachineOperand &MO) {
  for (unsigned reg = X86::YMM0; reg <= X86::YMM15; ++reg) {
    if (!MO.clobbersPhysReg(reg))
      return false;
  }
  for (unsigned reg = X86::ZMM0; reg <= X86::ZMM15; ++reg) {
    if (!MO.clobbersPhysReg(reg))
      return false;
  }
  return true;
}
```

The function is named "clobbers ALL ymm and zmm regs" but only inspects
YMM0-15/ZMM0-15. AVX-512 introduces YMM16-31 / ZMM16-31. A regmask that
preserves the upper bank is **wrongly** classified as "clobbers all," so
the caller treats the corresponding call as a clean boundary and may
suppress an otherwise-needed vzeroupper.

Also `isYmmOrZmmReg` (lines 122-125) only flags YMM0-15/ZMM0-15, and
`checkFnHasLiveInYmmOrZmm` similarly ignores the upper bank — combining
to make the entire upper bank invisible to the dirty-state analysis.

This is at minimum an AVX-SSE transition-penalty perf regression for code
that uses the upper bank; combined with inline-asm or unusual ABIs it can
become a correctness issue if user-visible YMM16-31 state leaks past a
caller that the analysis declared clean.

## Fix

Iterate `X86::YMM0` through `X86::YMM31` and `X86::ZMM0` through `X86::ZMM31`,
or use the `TargetRegisterInfo::regunits` iteration over the full register
class. Same change in `isYmmOrZmmReg` and `checkFnHasLiveInYmmOrZmm`.

Source-confirmed.
