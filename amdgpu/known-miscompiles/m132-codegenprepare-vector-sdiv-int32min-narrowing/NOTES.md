# m132: `AMDGPUCodeGenPrepare` vector scalarizer composes the m103 i64 sdiv `INT32_MIN / -1` narrowing per lane

*Discovery method: code inspection.*  Sibling shape to
[m103](../m103-lowersdivrem-i64-int32min-narrowing/NOTES.md) -- this
is the *vector* version: a v2i64 (or wider) sdiv with a per-lane
INT32_MIN numerator and a divisor splat of `-1` triggers the same
i64-narrowing miscompile on each affected lane.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUCodeGenPrepare.cpp:1488-1520`
scalarizes vector div/rem operations:

```cpp
if (auto *VT = dyn_cast<FixedVectorType>(Ty)) {
  NewDiv = PoisonValue::get(VT);

  for (unsigned N = 0, E = VT->getNumElements(); N != E; ++N) {
    Value *NumEltN = Builder.CreateExtractElement(Num, N);
    Value *DenEltN = Builder.CreateExtractElement(Den, N);

    Value *NewElt;
    if (ScalarSize <= 32) {
      NewElt = expandDivRem32(Builder, I, NumEltN, DenEltN);
      ...
    } else {
      NewElt = shrinkDivRem64(Builder, I, NumEltN, DenEltN);   // <-- composes m103
      if (!NewElt) {
        NewElt = Builder.CreateBinOp(Opc, NumEltN, DenEltN);
        if (auto *NewEltBO = dyn_cast<BinaryOperator>(NewElt))
          Div64ToExpand.push_back(NewEltBO);
      }
    }
    ...
    NewDiv = Builder.CreateInsertElement(NewDiv, NewElt, N);
  }
}
```

`shrinkDivRem64` (line 1343) then performs the per-lane
`getDivNumBits > 32` shrink that is the exact m103 bug.  When
the divisor is a constant splat such as `<i64 -1, i64 -1>`,
`divHasSpecialOptimization` bails out of the IR-level expansion,
*but the per-lane scalar `sdiv i64 %elt, -1` survives into SDAG*
where `LowerSDIVREM`
(`AMDGPUISelLowering.cpp:2415-2430`) applies the same buggy
narrowing per element.

For a lane whose numerator is `sext(INT32_MIN)` (33 sign bits) and
whose divisor is `-1` (64 sign bits), both `ComputeNumSignBits > 32`
gates pass.  The narrowed i32 op `sdiv 0x80000000, -1` is poison;
lowering wraps to `0x80000000`; the outer `SIGN_EXTEND` yields
`0xFFFFFFFF_80000000`.  The well-defined i64 result is `+2^31`
(`0x00000000_80000000`).

## How the buggy shape arises

A v2i64 sdiv whose lanes are each `sext` from i32 and whose divisor
is a literal `-1` splat:

```c
typedef long2 __attribute__((ext_vector_type(2))) v2i64;
v2i64 bug(int32_t a, int32_t b) {
  v2i64 num = (v2i64){(int64_t)a, (int64_t)b};
  return num / (v2i64)(int64_t)-1;   // a == INT32_MIN -> lane 0 wrong
}
```

ComputeNumSignBits sees 33 on lane 0 (sext(INT32_MIN)) and 64 on the
splat `-1`, both gates pass per-lane, narrowing fires per-lane.

## Reproducer

`reduced.ll` builds a v2i64 numerator with lane 0 = sext(INT32_MIN)
and lane 1 = sext(100), divides by literal `<i64 -1, i64 -1>`, and
stores the low and high halves of lane 0's quotient.

`bash known-miscompiles/run_ll_reproducer.sh known-miscompiles/m132-codegenprepare-vector-sdiv-int32min-narrowing/reduced.ll`:

```
[0] input=0x80000000 O0=0x80000000 O2=0x80000000 mismatch=false
[1] input=0x00000064 O0=0xffffffff O2=0x00000000 mismatch=true
any_mismatch=true
```

* Index `[0]` is the low32 of lane-0 `q` -- `0x80000000` either
  way: both the buggy narrowed lowering *and* the correct `0 - x`
  fold produce the same low half for INT32_MIN.
* Index `[1]` is the high32 of lane-0 `q` -- `0xFFFFFFFF` at O0
  (buggy `SIGN_EXTEND` of the narrowed i32 sdiv) vs `0x00000000`
  at O2 (correct `sub i64 0, x` after InstCombine's `sdiv x, -1`
  fold).

True i64 result on the host: `q[0] = +2147483648 = 0x00000000_80000000`.

## Why O0 vs O2 cleanly mismatches

* At O2, InstCombine sees the vector `sdiv <2 x i64> %num, splat(-1)`
  and folds it to `sub <2 x i64> zeroinitializer, %num` *before*
  AMDGPUCodeGenPrepare scalarizes.  No sdiv survives -- no narrowing.
* At O0, the literal-divisor `sdiv` reaches AMDGPUCodeGenPrepare
  intact.  The scalarizer at 1488-1520 splits per-lane, then defers
  each `sdiv i64 %elt, -1` to SDAG (via `divHasSpecialOptimization`
  bail-out at line 1346).  SDAG `LowerSDIVREM` then applies the m103
  narrowing per scalar lane, mis-lowering lane 0.

## Why this matters in the default pipeline

`AMDGPUCodeGenPrepare` runs in every codegen pipeline (both `-O0`
and `-O2`).  The vector scalarizer (1488-1520) fires for any
v{2,3,4,...}i64 div/rem.  Any source emitting a vector i64 `sdiv`
whose lanes can include `(INT32_MIN, -1)` after sign-extension --
e.g. HIP code dividing a `long2`/`long4` of small ints by a small
constant divisor that simplifies to `-1` -- gets a buggy lane.

The m103 NOTES.md already describes the scalar case.  This entry
documents that the IR-level vector scalarizer in
`AMDGPUCodeGenPrepare.cpp:1488-1520` compounds the bug: it manufactures
the per-lane scalar shape that m103 then mishandles, *and* it never
checks whether any single lane's `(num, den)` would be
`(INT32_MIN, -1)`.

## Suggested fix

Fix m103 at its sources (`AMDGPUISelLowering.cpp:2415-2430` and
`AMDGPUCodeGenPrepare.cpp:1343-1372`).  The scalarizer at
1488-1520 is correct on its own -- it just propagates whatever
`shrinkDivRem64` / SDAG `LowerSDIVREM` does.  Tightening the m103
gate from `> 32` to `> 33`, or adding the `!(isPowerOfTwo(LHS) &&
isAllOnes(RHS))` exclusion described in m103's NOTES, fixes both the
scalar and the vector cases.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`q[0].hi = 0xFFFFFFFF` at O0, `0x00000000` at O2). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same buggy lowering -- both `LowerSDIVREM` and the vector scalarizer have been in tree for many releases. |

## Why the fuzzer hasn't caught it

* The current FuzzX i64 sdiv emitter rarely pairs a v2i64 numerator
  with lane 0 = `sext(INT32_MIN)` and a literal splat divisor of
  `-1`.  Per `MEMORY.md` (Prefer-random-over-idioms), the right
  hook is to enrich the i32-constant pool with `INT32_MIN` and `-1`
  and let the random emitter pick them as lane fillers for a
  `sext`-fed v{2,4}i64 sdiv with a constant-splat divisor.
* Until m103 itself is fixed, both O0 and O2 pipelines apply the
  buggy narrowing for the *non-literal* divisor form (e.g. `sdiv
  <2 x i64> %num, %splat-of-loaded-(-1)`), so the O0-vs-O2 oracle
  collapses for that shape -- only the literal-`-1` divisor exposes
  the differential.
