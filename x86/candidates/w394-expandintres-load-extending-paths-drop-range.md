# w394 ExpandIntRes_LOAD extending / big-endian paths drop !range metadata

`ExpandIntRes_LOAD` is the integer-result expander for wide integer loads
(e.g. i128 -> two i64). It has three sub-paths:

1. Normal load -> dispatches to `ExpandRes_NormalLoad` (see w391).
2. `bitsLE(NVT)` extending load (sext / zext / ext) -> uses
   `DAG.getExtLoad(...)` for the lo half.
3. Big-endian -> two extending loads.

For paths (2) and (3) the function reads `MMOFlags` and `AAInfo` but never
reads `N->getRanges()`, and never passes a `Ranges` argument to
`DAG.getExtLoad(...)`. The metadata is silently dropped.

## Source

`llvm/lib/CodeGen/SelectionDAG/LegalizeIntegerTypes.cpp` lines 4465-4574:

```cpp
void DAGTypeLegalizer::ExpandIntRes_LOAD(LoadSDNode *N,
                                         SDValue &Lo, SDValue &Hi) {
  ...
  MachineMemOperand::Flags MMOFlags = N->getMemOperand()->getFlags();
  AAMDNodes AAInfo = N->getAAInfo();
  // ^^^ getRanges() not read.
  ...
  if (N->getMemoryVT().bitsLE(NVT)) {
    EVT MemVT = N->getMemoryVT();
    Lo = DAG.getExtLoad(ExtType, dl, NVT, Ch, Ptr, N->getPointerInfo(), MemVT,
                        N->getBaseAlign(), MMOFlags, AAInfo);
                     // ^^^ No Ranges argument; signature does not even have one.
    ...
  } else if (DAG.getDataLayout().isLittleEndian()) {
    Lo = DAG.getLoad(NVT, dl, Ch, Ptr, N->getPointerInfo(), N->getBaseAlign(),
                     MMOFlags, AAInfo);
                  // ^^^ Ranges defaults to nullptr.
    ...
    Hi = DAG.getExtLoad(ExtType, dl, NVT, Ch, Ptr,
                        N->getPointerInfo().getWithOffset(IncrementSize), NEVT,
                        N->getBaseAlign(), MMOFlags, AAInfo);
                     // ^^^ No Ranges argument.
  } else {
    Hi = DAG.getExtLoad(ExtType, dl, NVT, Ch, Ptr, N->getPointerInfo(), ...,
                        N->getBaseAlign(), MMOFlags, AAInfo);
    Lo = DAG.getExtLoad(ISD::ZEXTLOAD, dl, NVT, Ch, Ptr,
                        N->getPointerInfo().getWithOffset(IncrementSize), ...,
                        N->getBaseAlign(), MMOFlags, AAInfo);
                     // ^^^ Both: no Ranges argument.
  }
  ...
}
```

`SelectionDAG::getExtLoad(ExtType, ..., MMOFlags, AAInfo)`
(`SelectionDAG.h:1534-1539`) has no `MDNode *Ranges` parameter at all in this
non-MMO-taking overload - so the only way for this expander to forward
`Ranges` is to use the `MachineMemOperand *` overload at line 1540, which
takes a freshly-built MMO that carries the (narrowed) Range.

## Reproducer

Little-endian path:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i128 @load_i128(ptr %p) {
  %v = load i128, ptr %p, !range !0
  ret i128 %v
}

!0 = !{i128 0, i128 100}
```

(For pure-normal i128 loads on x86 the dispatch goes through
`ExpandRes_NormalLoad` - that's w391. To exercise THIS function directly,
arrange for `ISD::isNormalLoad` to be false; e.g. force a `sextload` /
`zextload` of a non-byte-aligned wide integer, which on x86 typically does
not reach this expander either because x86 lowers the smaller-than-i128
extending loads natively. The bug is still real on targets / inputs where
the extending wide-int load survives to type legalization, e.g. i256 from
i192 sextload, or any non-x86 target that hits this code with such a load.)

## Command

```
llc -O2 -mtriple=x86_64-unknown-linux-gnu i128_range.ll \
    -print-after=finalize-isel
```

## MIR

The i128 normal-load test dispatches to `ExpandRes_NormalLoad` (covered by
w391); the resulting MIR (no `!range !0` on either half) is shown there.
The extending and big-endian branches of `ExpandIntRes_LOAD` itself are not
reached by x86-64 -O2 with stock IR, but the source defect is identical:
local var `AAInfo` is read, local var for `Ranges` is missing, and the
`DAG.getExtLoad(...)` overload chosen has no `Ranges` parameter.

## Impact

Wide-integer extending loads (`zextload` / `sextload` / `extload` to a type
that needs expansion) silently lose `!range` metadata. This is the
integer-expander twin of w390 / w391 / w393 - same defect class, same
information loss, just on a colder x86 code path.

## Fix sketch

Switch from the `(PtrInfo, Align, MMOFlags, AAInfo)` overloads of
`getLoad`/`getExtLoad` to the `(MachineMemOperand *)` overloads, building a
fresh MMO via `MF.getMachineMemOperand(... AAInfo, NarrowedRanges)` where
`NarrowedRanges` is `N->getRanges()` truncated/clipped to the new memory
type's bit width. (Plain `N->getRanges()` will trip the bit-width assert at
`SelectionDAG.cpp:10541`.)
