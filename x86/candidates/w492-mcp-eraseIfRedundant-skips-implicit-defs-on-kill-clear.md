# MachineCopyPropagation::eraseIfRedundant kill clearing range includes PrevCopy

File: `llvm/lib/CodeGen/MachineCopyPropagation.cpp`,
function `MachineCopyPropagation::eraseIfRedundant` (lines 624-664).

## Pattern

The kill-clearing range in `eraseIfRedundant`:

```cpp
// llvm/lib/CodeGen/MachineCopyPropagation.cpp:650-652
  for (MachineInstr &MI :
       make_range(PrevCopy->getIterator(), Copy.getIterator()))
    MI.clearRegisterKills(CopyDst, TRI);
```

Iterates `[PrevCopy, Copy)`. **PrevCopy is included** because
`make_range(begin, end)` is half-open `[begin, end)`, and `begin` is
`PrevCopy->getIterator()`.

Consider PrevCopy itself with respect to `CopyDst`: if `CopyDst == Dst`
(forward redundancy case), PrevCopy defines `Dst` (= `CopyDst`). PrevCopy
would not have a `kill` use of `CopyDst` (it has a `def`). So
`clearRegisterKills(CopyDst, TRI)` on PrevCopy is a no-op in the common
case.

**However**, PrevCopy could carry implicit-uses of `CopyDst` with `isKill`
in cases where:
1. The COPY has `implicit-kill $CopyDst` from a prior tied-superreg pattern
   that the verifier accepts.
2. After a pseudo-expansion, the COPY operand layout includes implicit reads.

In those cases the kill on PrevCopy gets cleared too. That's probably fine
(the value is being redefined anyway by PrevCopy), but combined with the
asymmetric handling of Src (not cleared at all), the kill bookkeeping is
inconsistent.

The actual issue is that the range INCLUDES the boundary instruction
`PrevCopy` but EXCLUDES `Copy`. If we accept `PrevCopy`'s redefining context,
we should also handle `Copy`'s kill set BEFORE erase. The Src operand of
`Copy` may carry `kill`; this kill is being thrown away when `Copy` is erased
at line 660. See companion #490 for the Src-side analysis.

## Source citation

```
llvm/lib/CodeGen/MachineCopyPropagation.cpp:646-652
  DestSourcePair CopyOperands = *isCopyInstr(Copy, *TII, UseCopyInstr);

  MCRegister CopyDst = getDstMCReg(CopyOperands);
  assert(CopyDst == Src || CopyDst == Dst);
  for (MachineInstr &MI :
       make_range(PrevCopy->getIterator(), Copy.getIterator()))
    MI.clearRegisterKills(CopyDst, TRI);
```

Note also: the `assert(CopyDst == Src || CopyDst == Dst)` — `Src` and `Dst`
here are the arguments to `eraseIfRedundant(MI, Dst, Src)`. Per call site
(line 942): `eraseIfRedundant(MI, Dst, Src) || eraseIfRedundant(MI, Src, Dst)`.
In the first form, `CopyDst = Dst` (the param), and we're erasing `MI` (=Copy)
which was a redundant `Dst = COPY Src`. In the second form, `CopyDst = Src`
because `Copy` was `Src = COPY Dst` (reverse direction).

In the second form: we erase `Src = COPY Dst`. The kills of `Src` (the COPY's
def) in [PrevCopy, Copy) get cleared — that's the symmetric handling. But the
kill of `Dst` (the COPY's use) on `Copy` itself is similarly lost. The
asymmetry persists.

## Reproduction sketch

Same as #490 but reversed:

```mir
# RUN: llc -mtriple=x86_64-linux-gnu -run-pass=machine-cp -verify-machineinstrs %s
---
name: trigger_rev
tracksRegLiveness: true
body: |
  bb.0:
    liveins: $rbx, $rdx
    renamable $rax = COPY renamable $rbx
    renamable $rdx = LEA64r renamable $rax, 1, $noreg, 0, $noreg
    renamable $rbx = COPY killed renamable $rax
    RET 0, $rdx, $rbx
...
```

The second COPY (`$rbx = COPY killed $rax`) is recognized as the inverse of
the first (`$rax = COPY $rbx`). MCP erases it via `eraseIfRedundant(MI, Src,
Dst)` (line 942, second arm). The `killed $rax` flag is lost.

## Confidence

Medium. Same root cause as #490, different call arm. Both arms share the
asymmetric kill-flag handling.
