## Candidate: adjustLiveRegs ignores popStackAfter's iterator mutation

File: /home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Target/X86/X86FloatingPoint.cpp:978-988

### Reasoning
```
if (Kills && I != MBB->begin()) {
    MachineBasicBlock::iterator I2 = std::prev(I);
    while (StackTop) {
      unsigned KReg = getStackEntry(0);
      if (!(Kills & (1 << KReg)))
        break;
      popStackAfter(I2);    // <-- popStackAfter may MOVE I2 forward
      Kills &= ~(1 << KReg);
    }
}
```
`popStackAfter` (line 890) is documented to leave its `MachineBasicBlock::iterator &I`
pointing at the inserted/modified instruction. Crucially, when the preceding
instruction sets FPSW and a *following* instruction reads it (line 905-912),
`popStackAfter` advances its iterator past the FPSW reader before inserting the pop:
```
if (Next != MBB.end() && Next->readsRegister(X86::FPSW, ...))
  I = Next;
```
After the first iteration of the `while (StackTop)` loop, `I2` may now point at a
later instruction than `std::prev(I)`. The next iteration's `popStackAfter(I2)` will
operate at that advanced position; if more kills must happen, they will be placed
beyond instructions that may consume the FP stack value, corrupting the model.

A second concern: if `popStackAfter` advances `I2` past or to `I`, subsequent
modifications happen at/after `I`, leaving freed slots invisible to the manual
`freeStackSlotBefore(I, KReg)` calls at line 994 which insert *before* `I`.

### Repro sketch
A block whose live-out FP set requires killing two registers, where the most recent
FP instruction is a FUCOM*-style that sets FPSW and is immediately followed by an
FNSTSW reader. Have both FP registers dead at block exit so two kill iterations are
needed.

### Wrong outcome
The second pop is inserted at the wrong location (past the FPSW reader), which may
clobber ST0 before its FPSW result has been latched, producing a wrong condition-code
result; or, equivalently, the stack model diverges from the real stack contents in
the successor block.
