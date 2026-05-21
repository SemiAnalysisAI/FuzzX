# w234: X86OptimizeLEAs removeRedundantLEAs skips overflow check for non-imm AddrDisp (global/JTI offset)

## Summary

`X86OptimizeLEAsImpl::isReplaceable` checks that the result of merging two LEAs' address displacements fits in 32 bits — but only for the case when the use instruction's `AddrDisp` operand is an immediate. When the `AddrDisp` operand is a `MachineOperand::isOffset()`-class (GlobalAddress, BlockAddress, ConstantPoolIndex, etc.), no overflow check is done, and `removeRedundantLEAs` then adds the shift to a possibly-overflowing offset:

```c++
// removeRedundantLEAs, lines 686-691
MachineOperand &Op = MI.getOperand(MemOpNo + X86::AddrDisp);
if (Op.isImm())
  Op.setImm(Op.getImm() + AddrDispShift);
else if (!Op.isJTI())
  Op.setOffset(Op.getOffset() + AddrDispShift);          // <<< no isInt<32> check
```

vs `isReplaceable`, lines 475-479:

```c++
// Check that the new address displacement will fit 4 bytes.
if (MI.getOperand(MemOpNo + X86::AddrDisp).isImm() &&         // <<< Imm only
    !isInt<32>(MI.getOperand(MemOpNo + X86::AddrDisp).getImm() +
               AddrDispShift))
  return false;
```

For non-imm `AddrDisp` (e.g. a global symbol with a large offset already), the post-merge `Op.getOffset() + AddrDispShift` can exceed signed-32 range, producing an out-of-range relocation that:

- x86 `disp32` field is signed 32-bit, but the assembler/relocation will silently encode the low 32 bits of the wrapped value.
- This is a wrong-address bug whose visibility depends on whether the linker reports the truncation.

## Source location

`llvm/lib/Target/X86/X86OptimizeLEAs.cpp`

- `isReplaceable` lines 429-483 (overflow check only for `Op.isImm()`)
- `removeRedundantLEAs` lines 685-691 (applies offset add for non-imm without check)
- `removeRedundantAddrCalc` lines 504-578 has similar paths (line 569 uses `ChangeToImmediate(AddrDispShift)`, but for cases where the original was a Global the AddrDispShift is just the raw shift)

## Reproducer scaffold

```ll
target triple = "x86_64-unknown-linux-gnu"

@huge = external global [2147483640 x i8], align 1

define i64 @test(i64 %i) optsize {
  ; Two LEAs that point into the SAME global with large offsets
  %p1 = getelementptr [2147483640 x i8], ptr @huge, i64 0, i64 2147483600
  %p2 = getelementptr [2147483640 x i8], ptr @huge, i64 0, i64 2147483608
  %v1 = load i64, ptr %p1, align 1
  %v2 = load i64, ptr %p2, align 1
  %r = add i64 %v1, %v2
  ret i64 %r
}
```

At `-O2 -Os`, the X86OptimizeLEAs pass may emit a single LEA with the global offset and use a +shift in the second load. If the shift overflows int32, the relocation against `huge` will encode a wrong offset.

## Fix

```diff
   // Check that the new address displacement will fit 4 bytes.
-  if (MI.getOperand(MemOpNo + X86::AddrDisp).isImm() &&
-      !isInt<32>(MI.getOperand(MemOpNo + X86::AddrDisp).getImm() +
-                 AddrDispShift))
-    return false;
+  const MachineOperand &DispOp = MI.getOperand(MemOpNo + X86::AddrDisp);
+  if (DispOp.isImm() && !isInt<32>(DispOp.getImm() + AddrDispShift))
+    return false;
+  if (!DispOp.isImm() && !DispOp.isJTI() &&
+      !isInt<32>(DispOp.getOffset() + AddrDispShift))
+    return false;
```

## Severity

Less likely to fire than the imm-overflow case (which is rejected) because globals rarely have 2GB offsets. But for embedded targets or very-large-static-data scenarios, this is a silent miscompile.
