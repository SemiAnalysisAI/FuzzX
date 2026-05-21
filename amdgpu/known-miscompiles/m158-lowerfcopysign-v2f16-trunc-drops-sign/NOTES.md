# m158: `lowerFCOPYSIGN` v2f16/v2bf16 mag with v2f32 sign drops the sign bit via TRUNCATE

*Discovery method: code inspection (performFCopySignCombine audit).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:8817-8823`
(`SITargetLowering::lowerFCOPYSIGN`, v2f16/v2bf16 mag + wider sign
path):

```cpp
SDValue SignAsInt = DAG.getBitcast(MVT::v2i32, Sign);
SDValue SignI16   = DAG.getNode(ISD::TRUNCATE, ..., MVT::v2i16, SignAsInt);
//                  ^^^^^^^^^^ takes low 16 bits of each i32 ->
//                              drops bit 31 (sign), substitutes mantissa bit 15
SDValue SignF16   = DAG.getBitcast(MVT::v2f16, SignI16);
return DAG.getNode(ISD::FCOPYSIGN, ..., MagVT, Mag, SignF16);
```

`ISD::TRUNCATE` on `v2i32 -> v2i16` keeps the **low 16 bits** of
each i32 element, dropping bit 31 -- which is exactly where the f32
sign bit lives.  The subsequent `FCOPYSIGN v2f16, v2f16` then reads
bit 15 of the truncated value, which is **mantissa bit 15** of the
original f32, not its sign.

Result: the produced half/bf16 carries an essentially random sign
rather than the input f32's sign.

## Reachability

`performFCopySignCombine` (`SIISelLowering.cpp:13974`) peeks through
`FP_ROUND`/`FP_EXTEND` and otherwise returns `SDValue()` for non-f64
cases, so a raw mismatched `FCOPYSIGN v2f16, v2f32` reaches Custom
lowering.  All existing in-tree tests originate from `fptrunc` and
are simplified by the combiner before this path runs -- the bug is
**uncovered** by lit tests.

The fuzzer can hit it by generating `FCOPYSIGN` directly from
arbitrary v2f32 sign producers (loads, arith) without an explicit
fptrunc.

## Correct sequence

```cpp
// Shift sign bit from bit 31 to bit 15:
SDValue SignAsInt = DAG.getBitcast(MVT::v2i32, Sign);
SDValue SignShifted = DAG.getNode(ISD::SRL, ..., MVT::v2i32, SignAsInt,
                                   DAG.getConstant(16, ..., MVT::v2i32));
SDValue SignI16 = DAG.getNode(ISD::TRUNCATE, ..., MVT::v2i16, SignShifted);
SDValue SignF16 = DAG.getBitcast(MVT::v2f16, SignI16);
```

Equivalently, use `EXTRACT_ELEMENT` of the high half of each
`v2i32` lane.

## Why the fuzzer hasn't caught it

* Generic random IR usually generates copysign with same-type
  mag/sign or with `fptrunc`-wrapped sign that gets folded away.
  Per `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should construct `llvm.copysign.v2f16` with a sign argument that
  is a bitcast/load/arith producing v2f16 indirectly (e.g.
  `bitcast i32 -> v2f16` where the i32 carries f32 sign bits in
  unusual positions).

## Other paths audited (clean)

* `performFCopySignCombine` f64 split (lines 13990-14033) and
  sign-narrowing (14035-14067) look correct: high lane index
  `2*I+1` is little-endian high half of f64.
* NaN payload: `FCOPYSIGN` only manipulates sign bit; payload
  preserved on the mag operand path.
* bf16 FCOPYSIGN Legal (line 245) shares the f16 sign-bit position
  (bit 15); BFI mask `0x7fff7fff` works for both.
* gfx950 inherits the v2f16/v2bf16 Custom action (line 815), so
  the bug applies on gfx950.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Buggy TRUNCATE path emits wrong sign. |
| ROCm 7.1.1 | Same defect. |
