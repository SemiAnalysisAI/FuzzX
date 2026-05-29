# 002 — `llvm.minimumnum(sNaN, qNaN)` returns the raw sNaN

Component: SelectionDAG/DAGCombiner (generic, but in default x86 pipeline)

## Source

`llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp` :: `visitFMinMax`
(around lines 20498–20516):

```cpp
if (AF.isNaN()) {
  if (PropAllNaNsToQNaNs || (AF.isSignaling() && PropOnlySNaNsToQNaNs)) {
    if (AF.isSignaling())
      return DAG.getConstantFP(AF.makeQuiet(), SDLoc(N), VT);
    return N->getOperand(1);
  }
  return N->getOperand(0);   // <-- always returns X for *NUM intrinsics
}
```

`PropAllNaNsToQNaNs` is set for `ISD::FMINIMUM` / `ISD::FMAXIMUM`.
`PropOnlySNaNsToQNaNs` is set for `ISD::FMINNUM` / `ISD::FMAXNUM`.
For `ISD::FMINIMUMNUM` / `ISD::FMAXIMUMNUM` (the IEEE-754 2019
`minimumNumber`/`maximumNumber`), both flags are false, so the code falls into
the `return N0` branch for **any** NaN constant on the RHS — including when
the LHS is also NaN at runtime.

IEEE 754-2019 specifies that `minimumNumber(x, y)` returns a quiet NaN if
both x and y are NaN, and that sNaN payloads must be quieted on the result.
The DAGCombiner transform `minimumnum(X, qNaN) -> X` is therefore valid only
when `X` is known non-NaN (e.g. the `nnan` fast-math flag is present), but
the transform is applied unconditionally.

## Runtime demonstration

`repro.ll` defines `minimumnum_x_qnan` (f64) and a f32 variant. `runner.c`
feeds a signaling-NaN double (`0x7FF0000000000001`) and an sNaN float
(`0x7F800001`) and checks that the result has the quiet bit set.

Output from `./cmd.sh`:

```
input  sNaN double : 0x7ff0000000000001
result        f64  : 0x7ff0000000000001 (want: top bits 0x7FF8...; got 0x7FF0... = sNaN)
input  sNaN float  : 0x7f800001
result        f32  : 0x7f800001  (want: 0x7FC00001; got 0x7F800001 = sNaN)
FAIL f64: sNaN survived
FAIL f32: sNaN survived
```

The generated asm for `minimumnum_x_qnan` is literally `movq %xmm0, %rax;
retq` — i.e., the entire intrinsic call has been replaced by the identity.

## Why this is a bug

The IR-level intrinsics `llvm.minimumnum` / `llvm.maximumnum` are
documented as implementing IEEE-754 `minimumNumber`/`maximumNumber`. A user
who calls these expects sNaN inputs to be quieted; instead, with the
common idiom of using a qNaN sentinel, the sNaN passes straight through.

## Fix sketch

Guard the `return N0` branch on `(!N0CFP) ? N->getFlags().hasNoNaNs() : ...`
or, equivalently, when the opcode is `FMINIMUMNUM`/`FMAXIMUMNUM`, require
that the *other* operand is `nnan`-known-non-NaN before falling through.

## Files

- `repro.ll`  — IR
- `runner.c`  — drives the compiled object with sNaN inputs
- `cmd.sh`    — compiles & runs; non-zero exit means bug reproduced
