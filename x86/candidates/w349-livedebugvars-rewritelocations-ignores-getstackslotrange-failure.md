# w349: LiveDebugVariables::rewriteLocations silently uses SpillOffset=0 when getStackSlotRange fails — wrong debug location for subreg spills

## Status
CONFIRMED in source as a known FIXME. Debug-info miscompile (wrong variable location) when a subregister has no single representable offset within the spill slot.

## Source
`llvm/lib/CodeGen/LiveDebugVariables.cpp:1581-1597`
```cpp
} else if (VRM.getStackSlot(VirtReg) != VirtRegMap::NO_STACK_SLOT) {
  // Retrieve the stack slot offset.
  unsigned SpillSize;
  const MachineRegisterInfo &MRI = MF.getRegInfo();
  const TargetRegisterClass *TRC = MRI.getRegClass(VirtReg);
  bool Success = TII.getStackSlotRange(TRC, Loc.getSubReg(), SpillSize,
                                       SpillOffset, MF);

  // FIXME: Invalidate the location if the offset couldn't be calculated.
  (void)Success;

  Loc = MachineOperand::CreateFI(VRM.getStackSlot(VirtReg));
  Spilled = true;
}
```

## Description
`getStackSlotRange` is declared as:
```
virtual bool getStackSlotRange(const TargetRegisterClass *RC, unsigned SubIdx,
                               unsigned &Size, unsigned &Offset,
                               const MachineFunction &MF) const;
```
and the documentation explicitly says "subregisters registers may not be byte-sized, and a pair of discontiguous subregisters has no single offset" — so the function CAN return false.

In `rewriteLocations`, the return value is captured into `Success` but then deliberately discarded via `(void)Success`. The FIXME comment acknowledges the bug: the location should be invalidated if the offset could not be calculated.

Instead, the code:
1. Leaves `SpillOffset` at 0 (the value it was initialized to at line 1571).
2. Builds a `MachineOperand::CreateFI(VRM.getStackSlot(VirtReg))` pointing to the stack slot at offset 0.
3. Marks it `Spilled = true` and emits a DBG_VALUE with `frame-index` + offset 0.

For a subregister whose actual location within the spill slot is NOT offset 0 (e.g., the high half of a 64-bit reg on a little-endian target, or a discontiguous subreg pair), the debugger will report the WRONG bits as the variable's value. This is a debug-info miscompile.

## Severity
Wrong debug info under -g. No effect on emitted code, but `gdb` / `lldb` / debug-info consumers will display incorrect variable values. For optimized code under `-g`, the consequences include:
- Wrong values shown in watch windows.
- Wrong values used by `-fsanitize=address`'s symbol resolution callbacks.
- Wrong values reported in core dumps.

## Repro sketch
Targets where `getStackSlotRange` returns false: most affected are X86 with 80-bit FP register classes and AArch64 SVE / AMX. A small X86-targeting repro using `x87` long double subregs may suffice:

```ll
define x86_fp80 @f(x86_fp80 %x, x86_fp80 %y) {
entry:
  call void @llvm.dbg.value(metadata x86_fp80 %x, metadata !6, metadata !DIExpression()), !dbg !7
  ; high register pressure to force spill of %x
  %r = fadd x86_fp80 %x, %y
  ret x86_fp80 %r
}
```
Compile with `-O2 -g` and inspect the emitted DBG_VALUE in MIR after virtregrewriter; the FI operand may use offset 0 even when the actual location is at a higher byte offset within the slot.

## Fix sketch
Remove the `(void)Success;` and act on the failure:
```cpp
if (!Success) {
  // Cannot represent the subregister location within the spill slot;
  // invalidate the location for this user value.
  Loc.setReg(0);
  Loc.setSubReg(0);
} else {
  Loc = MachineOperand::CreateFI(VRM.getStackSlot(VirtReg));
  Spilled = true;
}
```
Note: the FIXME is in tree, so any patch fixing this also wants to delete the FIXME comment.
