# SCEVExpander::expandAddToGEP drops `inrange` when folding constant GEP base + constant index

File: `llvm/lib/Transforms/Utils/ScalarEvolutionExpander.cpp`
Function: `SCEVExpander::expandAddToGEP`, lines 386-437.

## The transform

When indvars asks SCEV to rewrite an exit-value LCSSA phi whose loop body uses
a pointer SCEV like `(8 + @vt) + 8*BTC`, the expander synthesizes the address
via `expandAddToGEP(Offset, V, Flags)`. If both `V` (the running pointer base)
and `Idx` (the expanded offset) are `Constant`s, line 397-399 takes the
constant-folding shortcut:

```cpp
// Fold a GEP with constant operands.
if (Constant *CLHS = dyn_cast<Constant>(V))
  if (Constant *CRHS = dyn_cast<Constant>(Idx))
    return Builder.CreatePtrAdd(CLHS, CRHS, "", NW);
```

`Builder.CreatePtrAdd` on constants ultimately routes through
`Constant::getGetElementPtr(...)`, which accepts `std::optional<ConstantRange>
InRange = std::nullopt` (see `Constants.h` line 1450-1474). The expander
passes **only `NW`** (the wrap flags) and **never the `InRange`** field — even
if `V` was itself a `ConstantExpr` GEP carrying `inrange(lo, hi)` (e.g., a
vtable slot constant). The new ptradd loses `inrange` entirely.

The non-constant fall-through at line 436 has the same problem:
```cpp
return Builder.CreatePtrAdd(V, Idx, "scevgep", NW);
```
This also synthesizes a fresh GEP with no inrange information, even when the
original IR had `getelementptr inbounds i8, ptr <inrange-constexpr-GEP>, ...`.

## Reproducer

`/tmp/scev-hunt/t1d.ll` (saved next to this report; copy of the IR below):

```llvm
target triple = "x86_64-unknown-linux-gnu"

@vt = constant [16 x ptr] zeroinitializer

define ptr @lcssa_addr_test(i64 %n) {
entry:
  br label %loop

loop:
  %i = phi i64 [ 0, %entry ], [ %i.next, %loop ]
  %off = shl i64 %i, 3
  %addr = getelementptr inbounds i8,
            ptr getelementptr inbounds inrange(-8, 24)
                  ([16 x ptr], ptr @vt, i64 0, i64 1),
            i64 %off
  %i.next = add nuw nsw i64 %i, 1
  %cond = icmp ult i64 %i.next, %n
  br i1 %cond, label %loop, label %exit

exit:
  %ret = phi ptr [ %addr, %loop ]
  ret ptr %ret
}
```

Run:
```
opt -passes=indvars -S t1d.ll
```

Produces:
```llvm
exit:
  %umax = call i64 @llvm.umax.i64(i64 %n, i64 1)
  %0 = shl i64 %umax, 3
  %scevgep = getelementptr i8, ptr @vt, i64 %0
  ret ptr %scevgep
```

The exit value `%scevgep` lost both `inbounds` AND the `inrange(-8, 24)`
qualifier from the original constexpr GEP base. (The math is right: the loop
runs at least once so the exit is `@vt + 8 + 8*(umax-1) = @vt + 8*umax`. The
flag/range data is what the expander silently discards.)

SCEV print confirms the AddRec is `{(8 + @vt)<nuw><nsw>,+,8}<%loop>` with
exit value `((8 * (1 umax %n)) + @vt)`. The `inrange` is a property of the
IR base GEP, not the SCEV node, so SCEV cannot rediscover it after expansion.

## Why this is in-scope for x86

Per LangRef, `inrange(lo, hi)` is a hard constraint: any GEP-derived address
outside the `[base+lo, base+hi)` interval is *poison*. Optimizations such as
`GlobalDCE`'s vtable splitting, WPD (whole-program devirtualization), and
some constant-folders rely on `inrange` to prove that a pointer cannot escape
its vtable slice. Dropping `inrange` in the exit-value rewrite means a
later use of `%scevgep` is no longer known to live within the original
vtable's interval — exposing the address to interprocedural folding that
would have been blocked by `inrange`, and potentially preventing
devirtualization or enabling unsound CSE across vtables.

Downstream x86 codegen merely lowers the resulting GEP, but the IR-level
loss-of-information caused here can cascade into wrong-answer machine code
after later transforms re-interpret the now-unrestricted pointer.

## Status: source-confirmed + IR repro

The repro above is concrete and reproducible. The miscompile path requires
a follow-up pass (WPD, or another inrange-relying optimization) on the
rewritten IR to observe wrong-answer codegen at `-O2`; this is plausible but
not yet demonstrated end-to-end. The lost-inrange and lost-inbounds in the
expanded GEP is mechanically certain from the code at lines 397-399 and 436
of `ScalarEvolutionExpander.cpp`.
