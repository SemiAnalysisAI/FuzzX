# w392 ScalarizeVecRes_LOAD drops !range metadata

`ScalarizeVecRes_LOAD` is the single-element vector-scalarization legalizer
for `LOAD` (e.g. `<1 x i64>` becomes `i64`). It reads
`N->getAAInfo()` and `N->getMemOperand()->getFlags()` but never reads
`N->getRanges()` and never forwards it to the resulting `DAG.getLoad(...)`.

Like the split case (w390), the element type of a scalarized vector load is
the original vector element type, so any `!range` on the load applies
unchanged to the scalar half - it can be forwarded verbatim with no
truncation / bit-width concerns.

## Source

`llvm/lib/CodeGen/SelectionDAG/LegalizeVectorTypes.cpp` lines 562-576:

```cpp
SDValue DAGTypeLegalizer::ScalarizeVecRes_LOAD(LoadSDNode *N) {
  assert(N->isUnindexed() && "Indexed vector load?");

  SDValue Result = DAG.getLoad(
      ISD::UNINDEXED, N->getExtensionType(),
      N->getValueType(0).getVectorElementType(), SDLoc(N), N->getChain(),
      N->getBasePtr(), DAG.getUNDEF(N->getBasePtr().getValueType()),
      N->getPointerInfo(), N->getMemoryVT().getVectorElementType(),
      N->getBaseAlign(), N->getMemOperand()->getFlags(), N->getAAInfo());
      // ^^^ Final `MDNode *Ranges = nullptr` argument omitted.

  ReplaceValueWith(SDValue(N, 1), Result.getValue(1));
  return Result;
}
```

The `SelectionDAG::getLoad(ISD::MemIndexedMode AM, ISD::LoadExtType, ...)`
signature it calls (`SelectionDAG.h:1546-1562`) takes
`const MDNode *Ranges = nullptr` as the final parameter and threads it into
`MachineMemOperand` (`SelectionDAG.cpp:10515-10517`).

For contrast, the sibling `ScalarizeVecRes_ATOMIC_LOAD` at line 550 of the
same file uses `N->getMemOperand()` directly (which carries Ranges inside
the MMO), so it does preserve the metadata.

## Reproducer

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i64 @scalar_v1i64(ptr %p) {
  %v = load <1 x i64>, ptr %p, align 8, !range !0
  %e = extractelement <1 x i64> %v, i32 0
  ret i64 %e
}

!0 = !{i64 0, i64 200}
```

## Command

```
llc -O2 -mtriple=x86_64-unknown-linux-gnu scalarize_load_range.ll \
    -print-after=finalize-isel
```

## MIR (after `finalize-isel`)

```
%1:gr64 = MOV64rm %0:gr64, 1, $noreg, 0, $noreg :: (load (s64) from %ir.p)
```

The MMO is `(load (s64) from %ir.p)` - notice the missing `!range !0`. A
scalar `load i64, ..., !range !0` baseline produces
`(load (s64) from %ir.p, !range !0)`.

## Impact

Any `<1 x iN>` (or single-lane scalarised through other paths) load with
`!range` loses the metadata on x86-64. Post-isel range-aware optimisations
(e.g. immediate-form selection that relies on KnownBits at MI level) become
pessimistic for the scalarised load.

## Fix sketch

Add `, N->getRanges()` as the final argument to the `DAG.getLoad(...)` call.
