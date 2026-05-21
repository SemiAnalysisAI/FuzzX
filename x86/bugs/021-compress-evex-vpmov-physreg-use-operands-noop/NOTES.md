## X86CompressEVEX: tryCompressVPMOVPattern misses mask uses through fallthrough

**File:** `llvm/lib/Target/X86/X86CompressEVEX.cpp:227-318`

### Reasoning

`tryCompressVPMOVPattern` rewrites:

```
vpmov*2m %xmmN, %k0   ; (erase)
kmov*    %k0, %eax    ; -> vmovmskp* %xmmN, %eax
```

To make this safe it must prove that `MaskReg` (the k-register) is not
used outside the rewrite window. Steps:

1. Scan from `MI` forward in the same MBB looking for the unique KMOV use.
2. After finding KMovMI, ensure no later use of the mask occurs in the
   same MBB (loop continues to scan).
3. Check `MRI->use_operands(MaskReg)` for any use whose parent MBB is
   not `&MBB` (line 307-309).

Issue 1 (cross-BB):

```cpp
for (const MachineOperand &MO : MRI->use_operands(MaskReg))
  if (MO.getParent()->getParent() != &MBB)
    return false;
```

This pass is post-regalloc (`setNoVRegs()`), so `MaskReg` is a **physical
register** (e.g., `$k0`). `MRI->use_operands(PhysReg)` for a physical
register returns nothing meaningful in post-RA MIR — physical register
def/use tracking is via the live-in/live-out sets and explicit operands,
not the `use_operands` iterator (which only tracks vreg uses). So this
cross-BB check is effectively **a no-op** post-RA.

Result: if `$k0` is defined by the VPMOV2M, read by the KMOV in MBB1,
and **also live-out** of MBB1 (read in a successor MBB before being
redefined), the pass will:

- Erase the VPMOV2M (so $k0 is never written).
- Convert the KMOV to a VMOVMSK (so $k0 is never written, even by the
  rewritten instr).

…leaving the successor MBB reading an uninitialized $k0. This is a
miscompilation.

### What's wrong

The use-check at line 307-309 does not actually work on physical
registers. The pass should additionally check `MBB.isLiveOut(MaskReg)`
(via the MBB's `liveouts()` after regalloc / using `LivePhysRegs`) before
performing the rewrite.

### MIR sketch

```mir
bb.0:
  liveins: $xmm0
  $k0 = VPMOVQ2MZ128kr $xmm0
  $eax = KMOVBrk $k0
  ; $k0 is still LIVE here
  JCC_1 %bb.2, 4, implicit $eflags
  JMP_1 %bb.1

bb.1:
  liveins: $k0
  $edx = KMOVBrk $k0          ; <-- reads $k0, which the rewrite kills
  ...

bb.2:
  ...
```

After the pass (current behavior, buggy):

```mir
bb.0:
  $eax = VMOVMSKPDrr $xmm0    ; $k0 never written
  ...
bb.1:
  $edx = KMOVBrk $k0          ; reads uninitialized $k0
```

### Fix

After the `MRI->use_operands` check (which is dead code for physregs),
or instead of it, build a `LivePhysRegs` for `MBB` and check that
`MaskReg` is not live-out:

```cpp
LivePhysRegs LPR(*TRI);
LPR.addLiveOuts(MBB);
if (LPR.contains(MaskReg))
  return false;
```
