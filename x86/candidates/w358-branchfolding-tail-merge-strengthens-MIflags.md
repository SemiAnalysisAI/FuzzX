# w358: BranchFolding tail-merge strengthens MI flags (nuw/nsw/nnan/...) on one path

## Pass / Target
- BranchFolding tail-merge (`-O2` default), x86_64
- llc: `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc` (LLVM 23.0.0git)

## Root cause

`MachineInstr::isIdenticalTo`
(`llvm/lib/CodeGen/MachineInstr.cpp:673-765`) does not inspect `getFlags()`
(IR-flag bitfield with `NoUWrap`, `NoSWrap`, `IsExact`, `FmNoNans`, `FmNoInfs`,
`FmNsz`, `FmArcp`, `FmContract`, `FmAfn`, `FmReassoc`, …). Two `ADD32rr`s that
differ only by `nuw`/`nsw`, or two `MULSSrr`s that differ only by `nnan`, are
treated as identical.

`BranchFolder::mergeCommonTails`
(`llvm/lib/CodeGen/BranchFolding.cpp:838-873`) does not call `mergeFlagsWith`
(`llvm/lib/CodeGen/MachineInstr.cpp:578-582`) or otherwise reconcile MI flags
when merging matching instructions across blocks. The kept-block's instruction
survives unmodified; the donor's instructions are deleted along with their
flags. If the kept block happened to carry a strict flag (e.g. `nuw`) and the
donor lacked it, the merged single instruction is now annotated `nuw` while
representing both control-flow paths — including the donor path where the
arithmetic could legitimately wrap.

## Reproducer (.ll)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(i1 %c, i32 %a, i32 %b) {
entry:
  br i1 %c, label %T, label %F
T:
  %x = add i32 %a, %b
  br label %done
F:
  %y = add nuw i32 %a, %b
  br label %done
done:
  %v = phi i32 [%x, %T], [%y, %F]
  ret i32 %v
}
```

Command:
```
llc -O2 -mtriple=x86_64-unknown-linux-gnu repro.ll -o - \
    -print-before=branch-folder -print-after=branch-folder
```

### Before BranchFolding
```
bb.1.T:
  $eax = ADD32rr $eax, $edx, ...               ; no nuw
bb.2.F:
  $eax = nuw ADD32rr $eax, $edx, ...           ; nuw
```

### After BranchFolding
```
bb.0.entry:
  TEST8ri $dil, 1, ...
  $eax = nuw ADD32rr $eax, $edx, ...           ; nuw, but represents BOTH paths
  RET 0, $eax
```

The single merged ADD now bears `nuw` on the T-path, where the original IR
only had a plain `add` (wrapping permitted).

## Equivalent FP demonstration (also reproduces)

```llvm
define float @f(i1 %c, float %a, float %b) {
entry:
  br i1 %c, label %T, label %F
T:
  %x = fmul float %a, %b           ; no nnan
  br label %done
F:
  %y = fmul nnan float %a, %b
  br label %done
done:
  %v = phi float [%x, %T], [%y, %F]
  ret float %v
}
```

After BranchFolding:
```
$xmm0 = nnan nofpexcept MULSSrr $xmm0, $xmm1, implicit $mxcsr
```

The merged `MULSSrr` carries `nnan` for both control-flow paths — including
the T-path where %a or %b could legitimately be NaN.

## Why this is a bug

MI flags (`NoUWrap`, `NoSWrap`, `FmNoNans`, etc.) are *assertions* about the
operation. Downstream consumers (e.g. `GISelValueTracking` at
`llvm/lib/CodeGen/GlobalISel/GISelValueTracking.cpp:338-339` uses
`MI.getFlag(NoSWrap)`/`getFlag(NoUWrap)` to refine known bits from sub;
`LegalizerHelper.cpp:2892` likewise) consult them. Telling later passes the
operation does not wrap (or its result is not NaN) on a control path where it
might wrap (or produce NaN) is unsound and can drive a later miscompile.

This is the "strengthen" direction. The opposite ("weaken" — drop `nuw`) is
safe; this bug is the asymmetric case.

The choice of kept block depends on the layout / predecessor heuristic in
`TryTailMergeBlocks`
(`llvm/lib/CodeGen/BranchFolding.cpp:980-998`), so the bug fires
unpredictably across small CFG perturbations.

## Suggested fix

In `mergeCommonTails`
(`llvm/lib/CodeGen/BranchFolding.cpp:838-873`) intersect the flags across all
merged instructions: for each merged op, set its flags to
`Acc &= Donor->getFlags()` (the assertions that hold on *every* incoming path).
This mirrors what `mergeFlagsWith` does for two ops (it currently OR-unions,
which is correct for "either input is OK" but BranchFolding needs AND — only
assertions present on *all* merged paths can survive).

Equivalently, extend `MachineInstr::isIdenticalTo` to require matching
`getFlags()` — but that would prevent the merge entirely, losing the
performance benefit. The intersect-in-merge fix is preferred.
