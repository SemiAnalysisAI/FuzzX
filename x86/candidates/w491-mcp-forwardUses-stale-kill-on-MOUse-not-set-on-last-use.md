# MachineCopyPropagation::forwardUses leaves stale `kill` flag clearing semantics

File: `llvm/lib/CodeGen/MachineCopyPropagation.cpp`,
function `MachineCopyPropagation::forwardUses` (lines 816-914).

## Pattern

A COPY's source is forwarded to a downstream user; both the old kill on the
copy destination AND any new kill on the forwarded register are cleared:

```cpp
// llvm/lib/CodeGen/MachineCopyPropagation.cpp:898-909
    MOUse.setReg(ForwardedReg);

    if (!CopySrcOperand.isRenamable())
      MOUse.setIsRenamable(false);
    MOUse.setIsUndef(CopySrcOperand.isUndef());

    LLVM_DEBUG(dbgs() << "MCP: After replacement: " << MI << "\n");

    // Clear kill markers that may have been invalidated.
    for (MachineInstr &KMI :
         make_range(Copy->getIterator(), std::next(MI.getIterator())))
      KMI.clearRegisterKills(CopySrc, TRI);
```

The kill-clearing range is `[Copy, MI]` (inclusive of MI via `std::next`).
This is necessary because the forwarded register `CopySrc` may have stale
"already killed" markers between Copy and MI from previous analysis. But the
loop unconditionally clears ALL kills of CopySrc — including any that were
originally on operands of MI (or even on Copy itself).

Pre-forwarding state:
```
Copy:    $rax = COPY killed $rbx           ; CopySrc=$rbx, killed at Copy
...
MIa:     ... use $rax (kill?) ...           ; intermediate use of Dst=$rax
...
MI:      $rcx = ADD killed $rax, ...        ; killed $rax → killed Dst
```

After forwarding `$rax` → `$rbx` at MI:
```
Copy:    $rax = COPY $rbx                   ; kill of $rbx cleared on Copy
...
MIa:     ... use $rax (kill?) ...           ; unchanged (no CopySrc on MIa)
...
MI:      $rcx = ADD $rbx, ...               ; was "killed $rax", became "$rbx"
                                            ; kill flag cleared too
```

Now $rbx is read at Copy (no kill) and at MI (no kill). No operand carries
`killed $rbx`. If MI is in fact the last use of $rbx, the static kill flag is
missing — same pattern as #490, except here it's the forwarding direction
rather than redundant-copy erasure.

## Why this matters

Note that `Copy` itself is NOT erased by `forwardUses`. It will become a
candidate for elimination only if no later instruction reads `$rax` (then
MaybeDeadCopies gets it). When Copy survives, the live range of `$rbx` is
correctly maintained by Copy's read at the top (Copy still reads $rbx). MI's
read of $rbx becomes the second/last use.

But if Copy is later erased as dead, MI's use of $rbx is the only use. Without
the kill flag, downstream analyses see $rbx as live past MI — a missed
optimization opportunity at best, a wrong reordering at worst.

## Source citation

```
llvm/lib/CodeGen/MachineCopyPropagation.cpp:906-909
  // Clear kill markers that may have been invalidated.
  for (MachineInstr &KMI :
       make_range(Copy->getIterator(), std::next(MI.getIterator())))
    KMI.clearRegisterKills(CopySrc, TRI);
```

The comment says "may have been invalidated" — recognizing the over-conservative
behavior. The fix is to instead transfer the kill from `Copy`'s `Source` operand
to `MOUse` if no other user of CopySrc exists in `(Copy, MI)`.

## Reproduction sketch (MIR)

```mir
# RUN: llc -mtriple=x86_64-linux-gnu -run-pass=machine-cp \
#      -verify-machineinstrs %s | FileCheck %s
---
name: trigger
tracksRegLiveness: true
body: |
  bb.0:
    liveins: $rbx
    renamable $rax = COPY renamable killed $rbx
    NOOP
    renamable $rcx = LEA64r renamable killed $rax, 1, $noreg, 8, $noreg
    RET 0, $rcx
...
```

Note: LEA64r is used as an example forward-target. After MCP, the second
operand of LEA becomes `$rbx`, but neither operand will carry `killed`. This
is verifier-safe (verifier doesn't catch missed kills) but degrades downstream
liveness tracking precision.

## Confidence

Medium. The behavior is intentional per the comment ("may have been
invalidated") but errs on the over-conservative side, dropping a kill that was
correctly placed on `Copy`'s source operand and not re-placing it elsewhere.
This is the source-level companion to the existing source-confirmed bug #095
(hasImplicitOverlap misses implicit-def of source).
