# MachineCopyPropagation::eraseIfRedundant loses kill flag on the source register

File: `llvm/lib/CodeGen/MachineCopyPropagation.cpp`,
function `MachineCopyPropagation::eraseIfRedundant` (lines 624-664).

## Pattern

Forward propagation hits a redundant identical copy:

```
PrevCopy: $rax = COPY $rbx           ; tracked, Src=$rbx not killed (rbx used later)
...
MI_b:     ... use $rbx, kill?         ; some use of $rbx in between
...
Copy:     $rax = COPY killed $rbx     ; nop-copy of PrevCopy, has kill on Src
```

`eraseIfRedundant` (line 624) erases `Copy` outright at line 660. The kill
clearing loop (line 650-652) only iterates over `CopyDst` (the destination
register), e.g.:

```cpp
  MCRegister CopyDst = getDstMCReg(CopyOperands);
  assert(CopyDst == Src || CopyDst == Dst);
  for (MachineInstr &MI :
       make_range(PrevCopy->getIterator(), Copy.getIterator()))
    MI.clearRegisterKills(CopyDst, TRI);
```

The kill that lived on `Copy`'s source operand (the `killed $rbx` above) is
**not transferred** to the last user of `$rbx` in the range `[PrevCopy, Copy)`,
nor to PrevCopy itself. After `Copy.eraseFromParent()`:

- If `$rbx` is not used anywhere else after `Copy` in this MBB, the last
  use of `$rbx` in the function is now wherever `MI_b` is (or PrevCopy's
  source operand), and neither carries an `isKill` flag.
- `$rbx`'s static kill set is now empty for this MBB even though the
  register dies somewhere here.

The companion `clearRegisterKills(CopyDst, TRI)` at line 652 is correct for
the destination — values redefined are no longer valid as "kills" after
removing one of the two identical defs. The same logic should be applied to
the source: the kill flag on Copy's `Src` operand needs to be moved to the
last surviving use of Src in `[PrevCopy, Copy)` (or set on the corresponding
operand of PrevCopy if no intervening use exists).

## Verifier impact

`MachineVerifier::checkLiveness` re-derives liveness from kill flags. A
missing kill on the last user of a physreg causes liveness to "leak" past
the actual last use, which then conflicts with the next defining instruction's
implicit dead/live state — generally producing
`Live range continues after operand killed it` or similar.

## Why this matters

The asymmetric treatment of Src vs. Dst kills produces a verifier-illegal MIR
even when no miscompile follows. Downstream consumers that trust kill flags
(MachineCSE, MachineSink in some configurations, MachineLICM's
post-RA-second-pass) can mis-extend live ranges.

There is also a real miscompile path: if a later pass uses kill-flag-driven
optimization (e.g. dead-COPY elimination based on "no kill flag → still live"
heuristics), it can keep a COPY around that should have been erased — or
conversely, fold an operation past a dead register believing it is still live.

## Source citation

```
llvm/lib/CodeGen/MachineCopyPropagation.cpp:644-652
  // Copy was redundantly redefining either Src or Dst. Remove earlier kill
  // flags between Copy and PrevCopy because the value will be reused now.
  DestSourcePair CopyOperands = *isCopyInstr(Copy, *TII, UseCopyInstr);

  MCRegister CopyDst = getDstMCReg(CopyOperands);
  assert(CopyDst == Src || CopyDst == Dst);
  for (MachineInstr &MI :
       make_range(PrevCopy->getIterator(), Copy.getIterator()))
    MI.clearRegisterKills(CopyDst, TRI);
```

## Reproduction sketch

```ll
; Run: llc -O2 -mtriple=x86_64-unknown-linux-gnu -verify-machineinstrs %s
; The MIR-level trigger requires two identical $rax = COPY $rbx with an
; intermediate user of $rbx and isKill on the second copy's source operand.
; Crafting from .ll is non-trivial because register allocator usually puts
; the second def somewhere that breaks the "identical" check. The cleanest
; trigger is MIR-direct (-run-pass=machine-cp).
```

```mir
# RUN: llc -mtriple=x86_64-linux-gnu -run-pass=machine-cp -verify-machineinstrs %s
---
name: trigger
tracksRegLiveness: true
body: |
  bb.0:
    liveins: $rbx
    renamable $rax = COPY renamable $rbx
    renamable $rcx = ADD64rr renamable $rbx, renamable $rbx, implicit-def dead $eflags
    renamable $rax = COPY killed renamable $rbx
    RET 0, $rax, $rcx
...
```

After MCP: the second COPY is erased, but the `killed` flag that was on its
$rbx operand vanishes, and no operand in the function carries the kill of
$rbx. The verifier should flag this.

## Confidence

Medium-high. The asymmetric handling of CopyDst vs. CopySrc kill flags is
visible at lines 650-652. The bug only manifests when `Copy` carries
`isKill` on its source AND that kill is not already present on PrevCopy
or an intermediate user. RA-produced MIR commonly has both copies of the
same nop kill-source.
