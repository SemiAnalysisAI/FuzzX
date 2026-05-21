# X86CleanupLocalDynamicTLS: `VisitNode` iterator semantics after `ReplaceTLSBaseAddrCall`/`SetRegister`

## File
`llvm/lib/Target/X86/X86CleanupLocalDynamicTLS.cpp`, lines 104-130.

## Code

```cpp
for (MachineBasicBlock::iterator I = BB->begin(), E = BB->end(); I != E;
     ++I) {
  switch (I->getOpcode()) {
  case X86::TLS_base_addr32:
  case X86::TLS_base_addr64:
    if (TLSBaseAddrReg)
      I = ReplaceTLSBaseAddrCall(*I, TLSBaseAddrReg);
    else
      I = SetRegister(*I, &TLSBaseAddrReg);
    Changed = true;
    break;
  ...
```

where:

```cpp
static MachineInstr *ReplaceTLSBaseAddrCall(MachineInstr &I, ...) {
  ...
  MachineInstr *Copy = BuildMI(*I.getParent(), I, ...);  // inserted *before* I
  I.eraseFromParent();                                    // I gone
  return Copy;
}

static MachineInstr *SetRegister(MachineInstr &I, Register *TLSBaseAddrReg) {
  ...
  MachineInstr *Next = I.getNextNode();
  MachineInstr *Copy = BuildMI(*I.getParent(), Next, ...); // inserted *after* I (before Next)
  return Copy;
}
```

## Bug

After `I = ReplaceTLSBaseAddrCall(...)`, `I` points to the newly-inserted `COPY` instruction, which sits *at the position the deleted `TLS_base_addr` used to occupy*. The loop then does `++I`. Two issues:

1. **The COPY itself is re-examined on the next iteration's `++I`**: actually no — `++I` advances past the COPY to the *next* MI, so the COPY is not re-scanned. OK.

2. **`SetRegister` returns the COPY inserted AFTER `I`**, and the loop body assigns `I = SetRegister(*I, ...)`. Then `++I` advances past the COPY. But the original `TLS_base_addr` instruction (`*I` on entry) is *not* erased — `SetRegister` only inserts the COPY after it. So `++I` skips over the COPY (correct, we don't want to rescan), but the loop has now moved one instruction *past* the position it would have been at if `SetRegister` had not inserted anything. Net effect: every instruction between the COPY and the original `*I`'s successor is **not skipped** — there's only one (the COPY itself), and skipping it is intentional.

Wait, re-reading: after `SetRegister`, `I` = the inserted COPY. The next iteration's `++I` advances to the instruction after the COPY, which is the *original* `TLS_base_addr`'s old successor (because the COPY was inserted between `*I_old` and its `Next`). But `*I_old` is *not erased* by `SetRegister`. So the original `TLS_base_addr` itself remains in the block, untouched, and the loop now sits past it.

**This means**: on the first encounter of `TLS_base_addr`, we call `SetRegister`, which inserts a `COPY` after the original instruction. The original `TLS_base_addr` is *kept* (it computes RAX/EAX, which the COPY then reads), and subsequent `TLS_base_addr` instructions in this block (or any dominated block) are replaced by `ReplaceTLSBaseAddrCall`. This is intentional. OK.

## Real concern

The `for` loop holds `E = BB->end()`. After `SetRegister`, the COPY is inserted before `Next`. If `*I` happens to be the last instruction in `BB` (i.e., `I.getNextNode() == nullptr` because there is no successor instruction in this block), then `SetRegister` calls `BuildMI(*I.getParent(), nullptr, ...)`. The `nullptr` insert-position to BuildMI with an MBB pointer means "append to end." The COPY then becomes the new last instruction. The loop's cached `E = BB->end()` was the *old* end (one past the original `TLS_base_addr`); after the insert, the new end is one further. Strictly, `BB->end()` is a sentinel that's stable across insertions, so `I != E` still terminates correctly when `I == end()`. But `++I` past the COPY yields `end()`, and the loop terminates. Fine.

However, if `*I` was a `TLS_base_addr` *terminator* (it shouldn't be, since it's modeled as a call-like compute), and `getNextNode()` returns `nullptr`, the BuildMI insert-point `Next=nullptr` is fine (appends).

## Status

No miscompile found. The pass is small and the iterator dance, while subtle, ends up correct. Documenting as ruled-out.

## Confidence

Ruled out after re-read.
