# Worker 540: Investigation notes for x86 stack-slot passes

Focus files (LLVM 23.0.0git):
- `llvm/lib/CodeGen/StackSlotColoring.cpp` (600 lines)
- `llvm/lib/CodeGen/StackColoring.cpp` (1381 lines)
- `llvm/lib/CodeGen/LocalStackSlotAllocation.cpp` (470 lines)

Default x86 `-O2` only.

## Scope filter: LocalStackSlotAllocation is unreachable from x86

`LocalStackSlotImpl::runOnMachineFunction` early-exits when
`!TRI->requiresVirtualBaseRegisters(MF)`
(LocalStackSlotAllocation.cpp:141-142). Only AMDGPU, ARM, AArch64,
RISCV and PowerPC override `requiresVirtualBaseRegisters`; **X86 does
not**. Any layout / alignment bug in LocalStackSlotAllocation is dead
on the default x86 pipeline and out of scope for this hunt. (Verified
by `grep -rn requiresVirtualBaseRegisters llvm/lib/Target/`.)

## Ruled-out leads (StackColoring.cpp)

- `Allocas[From] = To` then `From->comesBefore(To) → To->moveBefore(From)`
  (StackColoring.cpp:944-946). Wanted: break dominance of `To`'s size
  operand or `dbg.declare` ordering. Verdict: with opaque pointers all
  alloca operands are constants for the static-alloca cases used by
  frontend lifetime markers; dbg.declare-after-alloca is metadata, not
  dominance-sensitive at MIR level. Not a miscompile.
- `expungeSlotMap` while-loop chain length (StackColoring.cpp:1186-1192).
  Verdict: `SortedSlots[J] = -1` (line 1348) prevents any SecondSlot
  from ever becoming a FirstSlot, so the remap chain has length 1 and
  the while-loop terminates after one step. Not a bug.
- "BEGIN/END in same BB" handling (StackColoring.cpp:752-755). The
  BB-internal lifetime is captured by `calculateLiveIntervals`, not by
  the BEGIN/END BitVectors which only encode entering/leaving liveness.
  Resetting BEGIN if we then see END within the BB is correct.
- `BetweenStartEnd` initialization from predecessors during DFS
  (StackColoring.cpp:648-653). Back-edge predecessors may not yet have
  been visited, but this only over-marks slots as ConservativeSlots,
  which is sound (loses optimization, not soundness).
- `isLifetimeStartOrEnd` returning `false` after pushing the slot onto
  `slots` (StackColoring.cpp:596-624). Caller calls `slots.clear()`
  before each invocation (line 747), so the leftover push is harmless.

## Ruled-out leads (StackSlotColoring.cpp)

- `StackSlotColoring::ColorSlot` line 331-334 stack-ID mismatch branch.
  `Color` is taken from `UsedColors[StackID]` (line 320) which is set
  only when a color was assigned via that same StackID (line 341). The
  defensive recheck is dead code, not a bug.
- `RemoveDeadStores` `findRegisterUseOperandIdx(LoadReg, nullptr, true)`
  (line 509). The `isKill=true` flag is intentional: when the store
  doesn't kill `LoadReg`, the value is used past the store and we
  correctly delete only the store, leaving the load alive.
- Size/alignment update on shared color (line 354-359). Uses
  max-of-(new, current) under sharing, sets unconditionally when not
  sharing. `MFI->setObjectAlignment` (`MachineFrameInfo.h:524-533`)
  calls `ensureMaxAlignment` so the frame's MaxAlignment grows
  correctly when an AVX512-aligned spill shares with a smaller-aligned
  slot.
- `LoadReg != StoreReg` (line 502) catches subreg mismatches (`EAX` vs
  `RAX`) because the x86 `isLoadFromStackSlot` / `isStoreToStackSlot`
  early-reject any operand with a non-zero subreg index
  (`X86InstrInfo.cpp:686, 719`).
- Iterator advancement after debug-skip loop (lines 492-516) was
  carefully traced: `++I` past `end()` is avoided because we always
  `continue` when `NextMI == E` before the trailing `++I`.

## Verified empirically

- Loop with same-slot merges: `LIFETIME_START`/`END` in a loop body
  with two slots `a`, `b` correctly merges into one when disjoint
  (verified with `-print-after=stack-coloring` on synthetic IR).
- Different alignments: 1-byte alloca with `align 64` plus 64-byte
  alloca with `align 1` correctly produces one slot with size 64,
  align 64.
- Multiple LIFETIME_START for the same slot (across branches) and
  Itanium EH `invoke`/`landingpad`: still merges with disjoint slots.

## Tooling

- `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc`
  version 23.0.0git, host znver5, default target
  `x86_64-unknown-linux-gnu`.

## Summary

After ~30 minutes of focused investigation on these three files, **no
concrete miscompiles in StackColoring / StackSlotColoring on the
default x86 `-O2` pipeline were demonstrated**. The passes have been
hardened over time (PR27903 conservative-slot marking, SSP-layout
transfer, AAMD invalidation, scalable-stack-ID gating in
`contributesToMaxAlignment`). The most plausible code-smells (debug
instruction skip in dead-store removal, IR-level `comesBefore` move,
expungeSlotMap chain) reduce to over-conservatism or dead defensive
checks rather than miscompiles.

LocalStackSlotAllocation is unreachable from x86, so any layout
bugs there cannot manifest in the x86 pipeline.

No candidates emitted in 541-544.
