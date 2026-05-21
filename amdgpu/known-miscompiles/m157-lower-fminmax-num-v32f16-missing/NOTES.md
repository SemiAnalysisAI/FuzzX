# m157: `lowerFMINIMUMNUM_FMAXIMUMNUM` v32f16 missing handler case -> infinite loop / assert

*Discovery method: code inspection (during setOperationAction tables audit).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:826-829`
sets `ISD::FMINIMUMNUM` and `ISD::FMAXIMUMNUM` to `Custom` for the
vector types:

```cpp
setOperationAction({ISD::FMINIMUMNUM, ISD::FMAXIMUMNUM},
                   {MVT::v4f16, MVT::v8f16, MVT::v16f16, MVT::v32f16},
                   Custom);
```

`lowerFMINIMUMNUM_FMAXIMUMNUM` at `SIISelLowering.cpp:8637-8651` only
handles a subset:

```cpp
SDValue SITargetLowering::lowerFMINIMUMNUM_FMAXIMUMNUM(SDValue Op,
                                                      SelectionDAG &DAG) const {
  EVT VT = Op.getValueType();
  ...
  if (VT == MVT::v4f16 || VT == MVT::v8f16 || VT == MVT::v16f16 ||
      VT == MVT::v16bf16)
    return splitBinaryVectorOp(Op, DAG);
  return Op;          // <-- v32f16 falls through here
}
```

**v32f16 is missing** from the if-list.  In non-IEEE mode, a v32f16
op falls through to `return Op;` at line 8650, returning the
original Custom-marked node unchanged.  The legalizer treats this as
"no change" and either:

* enters an infinite legalization loop (legalizer keeps re-asking
  the Custom handler), or
* asserts with "Do not know how to legalize this operator!"
  depending on the legalizer iteration policy.

## Bonus defect

The handler lists `v16bf16`, but no `setOperationAction` in
lines 200-1000 marks `FMINIMUMNUM`/`FMAXIMUMNUM` Custom for any
bf16 vector -- that branch in the handler is dead code.

## Distinct from FMINNUM/FMAXNUM

The analogous `lowerFMINNUM_FMAXNUM` (line 8630) has the same
v32f16 omission, but `FMINNUM`/`FMAXNUM` for v32f16 is set `Expand`
(line 831), so it never reaches the Custom handler -- not a bug
there.  Only `FMINIMUMNUM`/`FMAXIMUMNUM` actually trigger the bad
fall-through.

## Reproducer

`reduced.ll`:

```llvm
declare <32 x half> @llvm.minimumnum.v32f16(<32 x half>, <32 x half>)

define amdgpu_kernel void @t(ptr addrspace(1) %p,
                             <32 x half> %a, <32 x half> %b) {
  %r = call <32 x half> @llvm.minimumnum.v32f16(<32 x half> %a, <32 x half> %b)
  store <32 x half> %r, ptr addrspace(1) %p
  ret void
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll`: either infinite
loop or assertion (depending on legalizer build).

## Suggested fix

Add `v32f16` to the splitter:

```cpp
if (VT == MVT::v4f16 || VT == MVT::v8f16 || VT == MVT::v16f16 ||
    VT == MVT::v32f16)
  return splitBinaryVectorOp(Op, DAG);
```

Remove `v16bf16` from the if (dead branch -- not marked Custom
anywhere).

OR alternatively, set `FMINIMUMNUM/FMAXIMUMNUM` to `Expand` for
v32f16 at line 826-829, matching what FMINNUM/FMAXNUM do.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits `llvm.minimumnum.v32f16` /
  `llvm.maximumnum.v32f16`.  Per `MEMORY.md`
  (Prefer-random-over-idioms), the random emitter should generate
  these IEEE-2019 min/max intrinsics with wide vector types
  including v32f16.
* The FuzzX validator restricts vector widths (per recent vecreduce
  agent's report); v32f16 falls outside the allowed pool.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Custom action set but handler missing case -> legalizer loop/assert. |
| ROCm 7.1.1 | Same defect. |

## Family

* m115/m124 (v2f16 fcanonicalize undef-lane).
* m141 (isCanonicalized bitcast loses fp-type).
* Same "setOperationAction Custom + handler missing case" family.
