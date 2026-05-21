## X86InsertVZeroUpper: `clobbersAllYmmAndZmmRegs` ignores YMM16-31 / ZMM16-31

**File:** `llvm/lib/Target/X86/X86InsertVZeroUpper.cpp:135-159`

### Reasoning

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

This is used by `hasYmmOrZmmReg` (lines 147-159) on call instructions:

```cpp
if (MI.isCall() && MO.isRegMask() && !clobbersAllYmmAndZmmRegs(MO))
  return true;   // call "dirties" YMM/ZMM
```

The intent is: a call that does NOT clobber every YMM/ZMM is treated as
producing/using YMM state (transition risk). But the loop only iterates
YMM0-15 and ZMM0-15. A call's regmask may clobber YMM0-15 but **not**
YMM16-31. In that case the regmask is reported as "clobbers all," and the
caller treats the call as harmless — yet AVX-512-aware code in the
caller may have live YMM16-31 values across the call, which the regmask
does NOT preserve.

Conversely: `isYmmOrZmmReg` (lines 122-125) only flags YMM0-15/ZMM0-15.
Combined, the pass simply *ignores* upper-bank registers entirely. Since
VZEROUPPER only touches YMM0-15, that's defensible — but the regmask
discriminator should be consistent with what live registers are *live*
across the call, not with what VZEROUPPER itself clears.

The current behavior: a call that clobbers YMM0-15 but preserves YMM16-31
will be classified as "clean," so the pass will not bother to look for
preceding YMM uses around it. That can suppress otherwise-needed
vzeroupper insertions when control later falls into SSE code, because the
call appears to be a clean boundary. Worse, the function may have live-in
YMM16-31 — `checkFnHasLiveInYmmOrZmm` also only looks at YMM0-15/ZMM0-15
— so live-in upper-bank YMMs are not registered as dirty entry state.

### Why it can be a bug, not just style

Consider an `__attribute__((target("avx512f")))` function that accepts a
`__m512i` argument in ZMM16 (LLVM does not generally pick this, but
inline-asm or attributes can construct such a call) and later transitions
to a region of SSE-encoded code. The dirty-tracking pass declares the
function clean at entry, and no VZEROUPPER is inserted, leading to the
classic SSE-AVX transition penalty (perf bug). Worse, on hardware that
doesn't fully respect upper-bank preservation across some kernel
transitions, this can corrupt state — though under normal user-space
calling conventions, YMM16-31 is caller-saved on Win64 / SysV AVX-512
extensions and we'd expect them clobbered.

### Severity

Performance regression / latent correctness depending on inline-asm and
target-attribute use. Worth at least a comment in the source explaining
the YMM0-15 restriction.

### MIR sketch

```mir
$ymm16 = AVX-512 producing instr ...
CALL64pcrel32 @sse_only_callee, <regmask preserving ymm16-31, clobbering ymm0-15>
$xmm0 = SSE instr reading something ...   ; transition penalty here
```
