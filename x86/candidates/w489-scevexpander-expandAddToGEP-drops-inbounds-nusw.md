# SCEVExpander::expandAddToGEP discards `inbounds`/`nusw` even when SCEV's `FlagNSW` is set

File: `llvm/lib/Transforms/Utils/ScalarEvolutionExpander.cpp`
Function: `SCEVExpander::expandAddToGEP`, lines 386-437.

## The transform

Pointer SCEV nodes can carry `FlagNUW` (no unsigned wrap of the address
arithmetic) and `FlagNSW` (no signed-i64 wrap). For pointer-typed AddRecs,
these correspond directly to GEP `nuw` and `nusw` respectively per LangRef
(GEP `nusw` says the signed-extended offset doesn't sign-wrap when added).
`inbounds` is implied by `nusw` plus the in-bounds geometric condition.

The expander's conversion is **lossy in two directions**:

```cpp
// Lines 392-394 (verbatim):
GEPNoWrapFlags NW = any(Flags & SCEV::FlagNUW)
                        ? GEPNoWrapFlags::noUnsignedWrap()
                        : GEPNoWrapFlags::none();
```

1. **`FlagNSW` is silently discarded.** The expander only inspects
   `FlagNUW`. A pointer SCEV with `<nsw>` (but not `<nuw>`) emerges as a
   plain `getelementptr i8, ...` with no `nusw` annotation. The GEP form
   `GEPNoWrapFlags::noUnsignedSignedWrap()` (which exists explicitly in
   `IR/GEPNoWrapFlags.h:53-55`) is never produced from `FlagNSW`.

2. **`inbounds` is never set.** Even when both `FlagNUW` and `FlagNSW`
   hold (which is the SCEV-level model of an `inbounds` GEP, because
   `inbounds` implies both no-unsigned and no-signed-wrap), the expander
   produces at most `noUnsignedWrap` — never `inBounds()`. This is
   visible in the repro from w485: the original IR
   `getelementptr inbounds i8, ptr <constexpr>, i64 %off` becomes the
   bare `getelementptr i8, ptr @vt, i64 %0` after expansion.

The caller at line 1370 even narrows the source flags before the call:

```cpp
return expandAddToGEP(SE.removePointerBase(S), StartV,
                      S.getNoWrapFlags(SCEV::FlagNUW));
```

`getNoWrapFlags(SCEV::FlagNUW)` masks down to only `FlagNUW`, so any
`FlagNSW` originally on the SCEV is dropped *before* even reaching
`expandAddToGEP`. The expander then drops `FlagNSW → nusw` once more on
the same line. This is double-loss of NSW.

## Reproducer (same as w485 — see lcssa exit-value rewrite path)

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

After `opt -passes=indvars -S`:

```llvm
exit:
  %umax = call i64 @llvm.umax.i64(i64 %n, i64 1)
  %0 = shl i64 %umax, 3
  %scevgep = getelementptr i8, ptr @vt, i64 %0
  ret ptr %scevgep
```

SCEV print earlier showed the AddRec carrying `<nuw><nsw>`. After
expansion the GEP carries **neither** `nuw`, `nusw`, nor `inbounds`. The
expanded form claims fewer wrap properties than the SCEV-level
representation knew.

## Why this is in-scope for x86

Per LangRef:
- `inbounds` enables the back-end to use signed-offset addressing modes
  without overflow checks.
- `nusw` enables sign-extension promotion of an i32 offset to i64 in the
  x86 address computation without needing a separate zext path.

Without these, x86 LSR and the address-mode selector can fail to fold an
offset into `[base + scaled_index + disp]`, producing extra `lea`/`add`
sequences. More importantly, when this expanded GEP feeds into a
subsequent SCEV re-analysis (the GEP is the new value of a phi that
re-enters another loop), SCEV's `getGEPExpr` (lines 3872+) only attaches
NUW/NSW when the source GEP has the corresponding flags. The flag loss
propagates into SCEV's model of the user, defeating downstream
optimizations.

## Related to w485

Both this and w485 are bugs in the same function. They differ in target:
- **w485**: the `inrange` ConstantRange on a constexpr base pointer is
  lost when both operands are Constants.
- **w489**: the wrap flags + `inbounds` are lost regardless of whether
  the operands are constants, on all SCEV-pointer-add expansions.

Both lose IR-level annotations that the source IR had explicitly, with
the same root cause: `expandAddToGEP` synthesizes a fresh GEP rather than
preserving or thoughtfully merging the source annotations.

## Status: source-confirmed (mechanical loss visible in code + IR repro).

The wrap-flag and `inbounds` loss is mechanically certain from inspection
of lines 392-394 and 1370. End-to-end x86 perf regression confirmable via
isel `-mtriple=x86_64` with/without the expander-emitted flags; wrong-
codegen requires the lost flags to be needed by a subsequent UB-based
optimization, which is plausible (SCEV NUW reasoning, LSR signed
addressing) but requires a chained pass to demonstrate.
