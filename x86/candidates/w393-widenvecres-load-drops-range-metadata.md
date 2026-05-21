# w393 WidenVecRes_LOAD / GenWidenVectorLoads drops !range metadata

`WidenVecRes_LOAD` widens illegal vector loads (e.g. `<3 x i32>` becomes
`<4 x i32>`, or `<6 x i32>` becomes one v4i32 load plus one v2i32 load). For
the non-VP, non-masked fallback path it calls `GenWidenVectorLoads` (also in
LegalizeVectorTypes.cpp). That helper reads `MMOFlags` and `AAInfo` from the
input load but never reads `LD->getRanges()`, and passes `nullptr` (default)
for the `Ranges` argument of every `DAG.getLoad(...)` it emits.

The element type of a widened vector load is identical to the original
(only the lane count changes), so `!range` applies unchanged to every part
that loads from real memory - it can be forwarded verbatim with no
truncation / bit-width assertion concern.

## Source

`llvm/lib/CodeGen/SelectionDAG/LegalizeVectorTypes.cpp` lines 8626-8714 (the
non-extending widening helper used by `WidenVecRes_LOAD`):

```cpp
SDValue DAGTypeLegalizer::GenWidenVectorLoads(SmallVectorImpl<SDValue> &LdChain,
                                              LoadSDNode *LD) {
  ...
  SDValue Chain = LD->getChain();
  SDValue BasePtr = LD->getBasePtr();
  MachineMemOperand::Flags MMOFlags = LD->getMemOperand()->getFlags();
  AAMDNodes AAInfo = LD->getAAInfo();
  // ^^^ getRanges() never read.
  ...

  SDValue LdOp = DAG.getLoad(*FirstVT, dl, Chain, BasePtr, LD->getPointerInfo(),
                             LD->getBaseAlign(), MMOFlags, AAInfo);
                          // ^^^ Ranges defaults to nullptr.
  ...
  for (EVT MemVT : MemVTs) {
    ...
    SDValue L =
        DAG.getLoad(MemVT, dl, Chain, BasePtr, MPI, NewAlign, MMOFlags, AAInfo);
                 // ^^^ Ranges defaults to nullptr.
    ...
  }
  ...
}
```

The `GenWidenVectorExtLoads` companion at line 8785 has the same omission
(scalar element extending loads).

The `SelectionDAG::getLoad(EVT, ...)` overload they call accepts
`const MDNode *Ranges = nullptr` as the final argument
(`SelectionDAG.h:1527-1531`), and the implementation
(`SelectionDAG.cpp:10515-10517`) plumbs it into the `MachineMemOperand`.

For contrast, the VP-load path of `WidenVecRes_LOAD` at line 6684 reuses
`LD->getMemOperand()` directly, which carries Ranges inside the MMO and
therefore preserves the metadata.

## Reproducer

```llvm
target triple = "x86_64-unknown-linux-gnu"

define <6 x i32> @widen_load_v6i32(ptr %p) {
  %v = load <6 x i32>, ptr %p, align 32, !range !0
  ret <6 x i32> %v
}

!0 = !{i32 0, i32 64}
```

## Command

```
llc -O2 -mtriple=x86_64-unknown-linux-gnu widen_load_range2.ll \
    -print-after=finalize-isel
```

## MIR (after `finalize-isel`)

```
%4:vr128 = MOVAPSrm %1:gr64, 1, $noreg, 0,  $noreg :: (load (s128) from %ir.p, align 32)
%5:gr64  = MOV64rm  %1:gr64, 1, $noreg, 16, $noreg :: (load (s64)  from %ir.p + 16, align 16)
```

Both halves lack the `!range !0` annotation.

## Impact

Any vector load that is widened (very common for unaligned counts like
`<3 x i32>`, `<6 x i32>`, `<5 x i64>`, ...) loses `!range` on x86. The
larger the post-widen load, the more cycles of MI-level range-aware
optimisation are starved of information.

## Fix sketch

Read `LD->getRanges()` once into a local `const MDNode *Ranges` (next to
`MMOFlags`/`AAInfo`) and pass it as the final argument to every
`DAG.getLoad(...)` call inside `GenWidenVectorLoads` and
`GenWidenVectorExtLoads`.
