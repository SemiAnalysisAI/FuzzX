# w390 SplitVecRes_LOAD drops !range metadata

`SplitVecRes_LOAD` reads `LD->getAAInfo()` and `LD->getMemOperand()->getFlags()`,
but never reads `LD->getRanges()`. It then calls `DAG.getLoad(AM, ExtType, ...)`
without supplying the `Ranges` parameter (which defaults to `nullptr`). The
result is that the `!range` metadata is silently dropped on both halves of the
split.

Vectors are different from integers here: the element type of a split-vector
load is identical to the original (e.g. `<4 x i64>` -> two `<2 x i64>`), so
the `!range` metadata applies unchanged to each half. There is no bit-width
mismatch and no truncation/assertion concern - the metadata can be forwarded
verbatim.

## Source

`llvm/lib/CodeGen/SelectionDAG/LegalizeVectorTypes.cpp` lines 2386-2430:

```cpp
void DAGTypeLegalizer::SplitVecRes_LOAD(LoadSDNode *LD, SDValue &Lo,
                                        SDValue &Hi) {
  ...
  MachineMemOperand::Flags MMOFlags = LD->getMemOperand()->getFlags();
  AAMDNodes AAInfo = LD->getAAInfo();
  // ^^^ getRanges() is never read.

  ...

  Lo = DAG.getLoad(ISD::UNINDEXED, ExtType, LoVT, dl, Ch, Ptr, Offset,
                   LD->getPointerInfo(), LoMemVT, LD->getBaseAlign(), MMOFlags,
                   AAInfo);
                // ^^^ no Ranges argument; defaults to nullptr.

  MachinePointerInfo MPI;
  IncrementPointer(LD, LoMemVT, MPI, Ptr);

  Hi = DAG.getLoad(ISD::UNINDEXED, ExtType, HiVT, dl, Ch, Ptr, Offset, MPI,
                   HiMemVT, LD->getBaseAlign(), MMOFlags, AAInfo);
                // ^^^ no Ranges argument; defaults to nullptr.
  ...
}
```

Compare with `SplitVecRes_VP_LOAD` immediately below it (line 2472), which
correctly forwards `LD->getRanges()` via the MMO constructor:

```cpp
MachineMemOperand *MMO = DAG.getMachineFunction().getMachineMemOperand(
    LD->getPointerInfo(), MachineMemOperand::MOLoad,
    LocationSize::beforeOrAfterPointer(), Alignment, LD->getAAInfo(),
    LD->getRanges());
```

`SelectionDAG::getLoad(AM, ExtType, ...)` signature
(`llvm/include/llvm/CodeGen/SelectionDAG.h:1546-1562`) takes an optional
`const MDNode *Ranges = nullptr` parameter; the implementation at
`llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp:10515` plumbs it through to
`MachineFunction::getMachineMemOperand(... Ranges)`.

## Reproducer

```llvm
target triple = "x86_64-unknown-linux-gnu"

define <4 x i64> @load_v4i64(ptr %p) {
  %v = load <4 x i64>, ptr %p, align 32, !range !0
  ret <4 x i64> %v
}

!0 = !{i64 0, i64 100}
```

## Command

```
llc -O2 -mtriple=x86_64-unknown-linux-gnu split_load_range.ll \
    -print-after=finalize-isel
```

## MIR (after `finalize-isel`)

```
%1:vr128 = MOVAPSrm %0:gr64, 1, $noreg, 0,  $noreg :: (load (s128) from %ir.p, align 32)
%2:vr128 = MOVAPSrm %0:gr64, 1, $noreg, 16, $noreg :: (load (s128) from %ir.p + 16, basealign 32)
```

Both MMOs lack the `!range !0` annotation that the source IR specifies. For
contrast, an equivalent scalar `load i64, ..., !range !0` lowers to:

```
%1:gr64 = MOV64rm ... :: (load (s64) from %ir.p, !range !0)
```

so the MIR machinery does propagate `!range` when it is given to `getLoad`.

## Impact

Lost `!range` blocks downstream optimizations that depend on it (e.g.
`MachineMemOperand::getRanges()` queries used by post-isel passes, alias
analysis, KnownBits at the MI level, scheduler heuristics). It is a
correctness-preserving loss of information, not a miscompile, but it makes
later x86 MI passes pessimistic for any vector wider than a legal type.

## Fix sketch

Read `LD->getRanges()` next to `getAAInfo()` and pass it as the last argument
to both `DAG.getLoad` calls.
