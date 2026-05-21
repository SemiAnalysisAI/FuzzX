# w391 ExpandRes_NormalLoad drops !range metadata

`ExpandRes_NormalLoad` is the entry point used by both `ExpandIntRes_LOAD`
(when `ISD::isNormalLoad(N)` is true) and `ExpandFloatRes_LOAD`
(for ppcf128/i128). The function reads `LD->getAAInfo()` and
`LD->getMemOperand()->getFlags()` but never reads `LD->getRanges()` and never
forwards it to the two `DAG.getLoad(...)` calls.

Unlike the vector cases, an integer expand DOES change element bit-width
(e.g. i128 -> two i64), so the original Range cannot be forwarded verbatim:
`SelectionDAG::getLoad` asserts "Range metadata and load type must match!"
(`SelectionDAG.cpp:10541-10545`). The fix has to truncate / clip the Range to
each half's bit width before forwarding. The current code does neither - it
just throws the metadata away.

## Source

`llvm/lib/CodeGen/SelectionDAG/LegalizeTypesGeneric.cpp` lines 246-283:

```cpp
void DAGTypeLegalizer::ExpandRes_NormalLoad(SDNode *N, SDValue &Lo,
                                            SDValue &Hi) {
  assert(ISD::isNormalLoad(N) && "This routine only for normal loads!");
  SDLoc dl(N);

  LoadSDNode *LD = cast<LoadSDNode>(N);
  assert(!LD->isAtomic() && "Atomics can not be split");
  EVT ValueVT = LD->getValueType(0);
  EVT NVT = TLI.getTypeToTransformTo(*DAG.getContext(), ValueVT);
  SDValue Chain = LD->getChain();
  SDValue Ptr = LD->getBasePtr();
  AAMDNodes AAInfo = LD->getAAInfo();
  // ^^^ no getRanges() / no MDNode *Ranges variable.

  ...

  Lo = DAG.getLoad(NVT, dl, Chain, Ptr, LD->getPointerInfo(),
                   LD->getBaseAlign(), LD->getMemOperand()->getFlags(), AAInfo);
  ...
  Hi = DAG.getLoad(NVT, dl, Chain, Ptr,
                   LD->getPointerInfo().getWithOffset(IncrementSize),
                   LD->getBaseAlign(), LD->getMemOperand()->getFlags(), AAInfo);
```

The `SelectionDAG::getLoad(EVT, ...)` overload it calls
(`SelectionDAG.h:1527-1531`) accepts `const MDNode *Ranges = nullptr` as the
final parameter.

## Reproducer

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i128 @load_i128(ptr %p) {
  %v = load i128, ptr %p, !range !0
  ret i128 %v
}

!0 = !{i128 0, i128 100}
```

## Command

```
llc -O2 -mtriple=x86_64-unknown-linux-gnu i128_range.ll \
    -print-after=finalize-isel
```

## MIR (after `finalize-isel`)

```
%1:gr64 = MOV64rm %0:gr64, 1, $noreg, 0, $noreg :: (load (s64) from %ir.p, align 16)
%2:gr64 = MOV64rm %0:gr64, 1, $noreg, 8, $noreg :: (load (s64) from %ir.p + 8, basealign 16)
```

Both halves have lost `!range !0`. A baseline i64 with !range shows
`(load (s64) from %ir.p, !range !0)`, so the lowering pipeline is
range-aware; only this legalizer path drops it.

## Impact

i128 (and other wide integer / ppcf128) loads with `!range` lose the metadata
on x86-64. Same story as w390, but for the integer-expand and float-expand
paths instead of the vector-split path - any post-isel range-aware
optimization sees nothing.

## Fix sketch

Truncate `LD->getRanges()` to the half-width and forward it to each
`DAG.getLoad` call. If the Range cannot be sensibly narrowed for one half
(e.g. it covers all of the high bits' possible values), pass `nullptr` for
that half only - never both.
