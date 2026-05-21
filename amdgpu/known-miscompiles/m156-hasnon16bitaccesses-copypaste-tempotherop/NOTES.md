# m156: `hasNon16BitAccesses` copy-paste bug -- `OpIs16Bit` check uses `TempOtherOp` width instead of `TempOp`

*Discovery method: code inspection (during zext/anyext combine
audit).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:14923-14924`:

```cpp
auto OpIs16Bit =
    TempOtherOp.getValueSizeInBits() == 16 || isExtendedFrom16Bits(TempOp);
//  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^                       (BUG)
```

Two lines down (line 14928-14929), the symmetric `OtherOpIs16Bit`
clause correctly uses `TempOtherOp` on both sides:

```cpp
auto OtherOpIs16Bit =
    TempOtherOp.getValueSizeInBits() == 16 || isExtendedFrom16Bits(TempOtherOp);
```

The `OpIs16Bit` check should test `TempOp.getValueSizeInBits()`, not
`TempOtherOp.getValueSizeInBits()`.  Classic copy-paste defect.

## Callsite

`performOrCombine` -> `matchPERM` at `SIISelLowering.cpp:15070`.  Used
to decide whether to lower `or`-tree patterns mixing `(zext i16)` and
larger ops into `v_perm_b32` (a byte-perm), versus keeping 16-bit
ops.

## Symptom

* **Case A (wrong-code direction)**: `Op` is genuinely 32-bit and
  `OtherOp` happens to be 16-bit.  `OpIs16Bit` becomes spuriously
  true.  The combine concludes "both are 16-bit", skips `v_perm`,
  and may leave a 16-bit-shape codegen for `Op` that does not model
  its actual width.  With zext semantics in the or-tree, this can
  drop the upper 16 bits of `Op` (lane-mix on i16->i32 zext when
  paired with an i16 OtherOp under an or, or v2i16->v2i32 patterns).
* **Case B (lost optimization)**: `OtherOp` is 32-bit and `Op` is
  16-bit/extended.  `OpIs16Bit` is forced false, the function
  returns true, and `v_perm` is always selected -- losing a valid
  16-bit-shape codegen opportunity.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %in, ptr addrspace(1) %out) {
  %xi = load i32, ptr addrspace(1) %in
  %h  = trunc i32 %xi to i16
  %z  = zext i16 %h to i32      ; OtherOp = 16-bit zext
  %m  = and i32 %xi, 65280
  %s  = shl  i32 %m, 8          ; Op = 32-bit
  %p  = or i32 %z, %s
  store i32 %p, ptr addrspace(1) %out
  ret void
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll`: the or-tree
reaches `matchPERM`; the buggy `hasNon16BitAccesses` returns the
wrong answer for this mixed-width shape.

## Suggested fix

```cpp
auto OpIs16Bit =
    TempOp.getValueSizeInBits() == 16 || isExtendedFrom16Bits(TempOp);
//  ^^^^^^^^                                                       (fix)
```

Same change is needed in the ROCm fork at
`amdgpu/third_party/rocm-llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:14978-14979`.

## Why the fuzzer hasn't caught it

* Generic random IR usually emits or-trees of uniform width.  Per
  `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should explicitly mix widths in or-trees (16-bit zext + 32-bit
  shifted operand) to surface matchPERM defects like this one.
* The defect manifests as a subtle change in which byte-perm
  selector is chosen, not always as a value miscompile, so the
  O0-vs-O2 oracle may not always catch it on a per-input basis.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Copy-paste defect present. |
| ROCm 7.1.1 (`rocm-llvm-project`) | Same defect at `:14978-14979`. |
