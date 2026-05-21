# w359: BranchFolding HoistCommonCodeInSuccs propagates TBB's flags / MMOs as-is, silently dropping FBB's

## Pass / Target
- BranchFolding `HoistCommonCodeInSuccs` (`-O2` default), x86_64
- llc: `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc` (LLVM 23.0.0git)

## Root cause

`BranchFolder::HoistCommonCodeInSuccs`
(`llvm/lib/CodeGen/BranchFolding.cpp:1955-2172`) finds the longest lockstep
identical prefix of the two single-pred successors `TBB` and `FBB` of an MBB
that ends in a conditional branch. It then does:

```
if (TBB == FBB)
  MBB->splice(Loc, TBB, TBB->begin(), TIB);     // single-block case
else
  for (TI : range(TBB)) { ...; TI->moveBefore(&*Loc); ++FI; }   // line 2133-2162
FBB->erase(FBB->begin(), FIB);                  // line 2165
```

Crucially:
- only `TBB`'s instructions are moved (their MMOs/flags/MIflags survive
  unchanged),
- `FBB`'s matching instructions are *erased* — their MMOs, MI flags
  (`nuw`/`nsw`/`nnan`/...), and extra info (`!pcsections`, `!mmra`) are simply
  dropped on the floor,
- no equivalent of `cloneMergedMemRefs` (used by the tail-merge path in
  `mergeOperations`, `llvm/lib/CodeGen/BranchFolding.cpp:821-822`) is invoked.

The lockstep check (`TIB->isIdenticalTo(*FIB, MachineInstr::CheckKillDead)`,
line 1994) does *not* compare MI flags, MMOs, PCSections, or MMRAs (see
`MachineInstr::isIdenticalTo`,
`llvm/lib/CodeGen/MachineInstr.cpp:673-765`), so the hoist proceeds even when
the TBB and FBB instructions disagree on those fields.

Effect: whichever path's instruction is `TBB` keeps its annotations; the
opposite path's annotations are silently lost. If TBB carries a *stronger*
assertion (`nuw`, `nnan`, ...) the merged instruction now bears that
assertion on the F-path also, which may not be valid there.

## Reproducer (.ll) — MIflag strengthening via Hoist

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(i1 %c, i32 %a, i32 %b) {
entry:
  br i1 %c, label %T, label %F
T:
  %x = add i32 %a, %b               ; no nuw
  %r = xor i32 %x, 1
  br label %done
F:
  %y = add nuw i32 %a, %b           ; nuw
  %s = xor i32 %y, 2
  br label %done
done:
  %v = phi i32 [%r, %T], [%s, %F]
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
bb.1.T:                                       ; this is FBB in analyzeBranch (false target)
  $eax = ADD32rr $eax, $edx, ...              ; plain ADD (no nuw)
  $eax = XOR32ri $eax, 1, ...
  JMP_1 %bb.3
bb.2.F:                                       ; this is TBB (true target of JCC)
  $eax = nuw ADD32rr $eax, $edx, ...          ; nuw ADD
  $eax = XOR32ri $eax, 2, ...
```

### After BranchFolding (Hoist fired)
```
bb.0.entry:
  $eax = COPY $esi
  $eax = nuw ADD32rr $eax, $edx, ...          ; <-- hoisted, KEEPS nuw
  TEST8ri $dil, 1, ...
  JCC_1 %bb.2, 4, ...
```

The merged hoisted ADD bears `nuw`. The T path (bb.1) originally had a plain
`add` — wrapping was allowed there. The merged instruction now asserts no
unsigned wrap on both paths, including the T-path where it may be untrue.

## Suggested fix

In `HoistCommonCodeInSuccs`, after each pair `(TI, FI)` of matched
instructions is found but before `TI->moveBefore(&*Loc)`:
1. Intersect MI flags: `TI->setFlags(TI->getFlags() & FI->getFlags())` (only
   assertions present on *both* paths survive). Mirrors what's needed in the
   tail-merge path too (w358).
2. Merge MMOs for memory ops: `TI->cloneMergedMemRefs(MF, {&*TI, &*FI})` so
   the hoisted op's memoperands list is the union (same fix as the tail-merge
   path uses, but currently absent from Hoist).
3. Conservatively drop / union PCSections, MMRA, HeapAllocMarker when they
   differ (also see w357).

Without these merges, the hoist can silently strengthen MI flags or drop
memory-effect metadata that downstream passes consume.
