# emitPCMPESTR: load fold places InGlue *after* chain operand, breaking EAX/EDX live-in glue ordering on some paths

**File**: `llvm/lib/Target/X86/X86ISelDAGToDAG.cpp:4424-4453`

## Reasoning

`emitPCMPESTR` accepts `InGlue` from two glued `CopyToReg` nodes (EAX
and EDX preload). The fold-load path builds:

```cpp
SDValue Ops[] = { N0, Tmp0, Tmp1, Tmp2, Tmp3, Tmp4, Imm,
                  N2.getOperand(0), InGlue };
SDVTList VTs = CurDAG->getVTList(VT, MVT::i32, MVT::Other, MVT::Glue);
MachineSDNode *CNode = CurDAG->getMachineNode(MOpc, dl, VTs, Ops);
InGlue = SDValue(CNode, 3);
// Update the chain.
ReplaceUses(N2.getValue(1), SDValue(CNode, 2));
// Record the mem-refs
CurDAG->setNodeMemRefs(CNode, {cast<LoadSDNode>(N2)->getMemOperand()});
```

The operand list ends with `N2.getOperand(0)` (the load's chain) and
`InGlue`. The load's chain may itself depend on the entry node, while
the EAX/EDX CopyToRegs that produce `InGlue` were chained off
`CurDAG->getEntryNode()` (line 6488/6491, in the PCMPESTR Select
branch). The MachineSDNode therefore carries:
  - chain operand = N2's chain (load chain)
  - glue operand = InGlue from CopyToReg(EAX,EDX)

Two issues:

1. **Memref / chain provenance lost for EAX/EDX preload.** The CNode's
   chain is the load chain only; the CopyToReg chain (which sequenced
   EAX/EDX writes) is reachable only via glue. If any later
   scheduling/peephole drops the glue edge (post-isel passes
   sometimes do for `MVT::Glue` that isn't observed), the EAX/EDX
   writes may be reordered relative to the PCMPESTR's memory load.
   That can let an aliased store between the CopyToReg and the
   PCMPESTR be moved before the implicit EAX/EDX live-in.

2. **`InGlue = SDValue(CNode, 3)` is assigned but the caller already
   has CNode as its result.** The chain-update happens via
   `ReplaceUses(N2.getValue(1), SDValue(CNode, 2))` — but `Sc->getChain()`
   for PCMPESTR is `CurDAG->getEntryNode()`, *not* `N2.getOperand(0)`.
   So users of the original PCMPESTR chain (the entry node) are not
   moved onto `SDValue(CNode, 2)`. Result: a downstream operation that
   was sequenced after PCMPESTR via the entry chain stays sequenced
   after the entry node only, while CNode sits with a *separate* chain
   rooted in N2's load chain. If the load N2 is from a memory location
   that aliases a later store, the scheduler is free to reorder them.

The mirror function `emitPCMPISTR` (line 4392-4419) has the same
shape but PCMPISTR doesn't take EAX/EDX live-in so item (1) doesn't
apply there.

## Repro sketch

```ll
define i32 @f(<16 x i8> %a, i32 %la, ptr %b_ptr, i32 %lb, ptr %store_ptr) {
  store i32 42, ptr %store_ptr               ; potential alias with %b_ptr
  %b = load <16 x i8>, ptr %b_ptr
  %r = call i32 @llvm.x86.sse42.pcmpestri128(<16 x i8> %a, i32 %la,
                                             <16 x i8> %b, i32 %lb, i8 0)
  ret i32 %r
}
```

Compile `-mattr=+sse4.2 -O2` and inspect whether the store can be sunk
past the `pcmpestrm` memory operand fold.

## Wrong outcome

EAX/EDX live-in CopyToRegs scheduled out of order with the PCMPESTR's
memory load; potential miscompile if the load's address aliases the
preceding store. At minimum, MMO miss-tracking can cause AA-aware
later passes to incorrectly NoAlias an aliasing store.

## Cross-reference

`llvm/test/CodeGen/X86/pr11334.ll`, `pcmp-cnt-test.ll` and
`sse42-intrinsics-fast-isel-x86_64.ll` exercise PCMPESTR but not the
fold-with-aliasing-store scenario.
