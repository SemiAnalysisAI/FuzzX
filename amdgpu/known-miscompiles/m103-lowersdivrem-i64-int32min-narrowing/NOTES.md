# m103: `LowerSDIVREM` i64 sign-shrink fast-path mishandles `INT32_MIN / -1`

*Discovery method: code inspection.* Sibling shape to m040
(`LowerDIVREM24` numeric corner) but distinct: this is the
i64-narrowing fast path admitting an i32 sdiv-overflow case.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:2415-2430`:

```cpp
if (VT == MVT::i64 &&
    DAG.ComputeNumSignBits(LHS) > 32 &&
    DAG.ComputeNumSignBits(RHS) > 32) {
  // ... shrink to i32 SDIVREM ...
  SDValue DIVREM = DAG.getNode(ISD::SDIVREM, DL,
                               DAG.getVTList(HalfVT, HalfVT),
                               LHS_Lo, RHS_Lo);
  SDValue Res[2] = {
    DAG.getNode(ISD::SIGN_EXTEND, DL, VT, DIVREM.getValue(0)),
    DAG.getNode(ISD::SIGN_EXTEND, DL, VT, DIVREM.getValue(1))
  };
  return DAG.getMergeValues(Res, DL);
}
```

`> 32` admits `LHS = sext(INT32_MIN)` (33 sign bits) and
`RHS = sext(-1)` (64 sign bits).  The narrowed i32 operation
`sdiv 0x80000000, -1` is i32-overflow / poison.  In practice the
lowering wraps to `0x80000000`; the outer `SIGN_EXTEND` then yields
`0xFFFFFFFF_80000000` = `-2^31`.

The well-defined i64 result is `+2^31` (`0x00000000_80000000`).

The same bug appears in
`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUCodeGenPrepare.cpp:1354`
(`shrinkDivRem64`) and is implemented by
`AMDGPUCodeGenPrepare::expandDivRem32`
(`AMDGPUCodeGenPrepare.cpp:1219`).  Because both O0 and O2 pipelines
narrow the divrem, they agree wrong -- which is why the FuzzX
O0-vs-O2 oracle has not caught it.

## How the buggy shape arises

Any IR that produces an i64 `sdiv` whose operands are `sext` from i32
of values that happen to include `INT32_MIN` and `-1`.  This is one
line of C:

```c
int64_t bug(int32_t x) { return (int64_t)x / (int64_t)-1; }  // x == INT32_MIN
```

ComputeNumSignBits sees 33 (>32) on `sext(x)` and 64 on `sext(-1)`,
both gates pass, narrowing fires.

## Reproducer

`reduced.ll` (also in this directory) feeds `INT32_MIN` and `-1`
through volatile loads + `sext i32 to i64`, then takes
`q = sdiv i64 LHS, RHS` and stores low and high halves of `q`.

`bash known-miscompiles/run_ll_reproducer.sh known-miscompiles/m103-lowersdivrem-i64-int32min-narrowing/reduced.ll`:

```
input=0x80000000
O0=0xffffffff      # buggy (narrowed lowering)
O2=0xffffffff      # buggy (narrowed lowering)
```

True i64 result on the host: `q = 2147483648`, low32 = `0x80000000`,
high32 = `0x00000000`.  The kernel stores `0x80000000` for both halves
on O0 and O2 -- the high32 is `0xFFFFFFFF` from the `SIGN_EXTEND`.

To get a clean O0-vs-O2 differential, force the RHS to be a literal
`-1` (so InstCombine can fold `sdiv x, -1 -> 0 - x` on O2 but not on
O0):

```llvm
%q = sdiv i64 %lo64, -1
```

Result: `O0=0xffffffff, O2=0x00000000, mismatch=true`.

## Suggested fix

Tighten the gate to `> 33` (the LHS needs at least 34 sign bits so
INT32_MIN cannot occur) or add an explicit special case:

```cpp
if (VT == MVT::i64 &&
    DAG.ComputeNumSignBits(LHS) > 32 &&
    DAG.ComputeNumSignBits(RHS) > 32 &&
    // Reject the only i32 sdiv-overflow case.
    !(DAG.isKnownToBeAPowerOfTwo(LHS) &&
      isAllOnesConstant(RHS))) {
  ...
}
```

A safer formulation: only enter the fast path when at least one of the
operands has >33 sign bits (so the i32 sdiv cannot hit INT32_MIN/-1).
The same fix needs to be replicated in `AMDGPUCodeGenPrepare.cpp:1219`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`q.hi = 0xFFFFFFFF`, true result has `0x00000000`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same buggy lowering. |

Not a HEAD-only regression -- both `LowerSDIVREM` and the
`AMDGPUCodeGenPrepare` narrowing have been in tree for many releases.

## Why the fuzzer hasn't caught it

* The O0-vs-O2 differential collapses because both pipelines apply the
  buggy narrowing.  The interpreter oracle (`scripts/interp`) is
  needed to flag this -- or one side must skip
  `AMDGPUCodeGenPrepare`.
* The current FuzzX i64 sdiv emitter rarely pairs `sext` from i32 with
  the specific `(INT32_MIN, -1)` input pair.  Per `MEMORY.md`
  (Prefer-random-over-idioms), the right hook is to add `INT32_MIN`
  and `-1` to the i32 constant pool and let the random emitter pick
  them as both operands of a `sext`-fed i64 sdiv.
