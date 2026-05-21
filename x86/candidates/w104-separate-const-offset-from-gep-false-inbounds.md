# w104: SeparateConstOffsetFromGEP::swapGEPOperand falsely marks GEP `inbounds`

## Status: confirmed (smoking gun: IR with a definitely-out-of-bounds `getelementptr inbounds`)

## Summary

`SeparateConstOffsetFromGEP::swapGEPOperand`
(`llvm/lib/Transforms/Scalar/SeparateConstOffsetFromGEP.cpp:1508-1530`)
swaps the offsets of two chained GEPs (`p+o+c` → `p+c+o`) so that the
loop-invariant constant `c` ends up on the outer GEP and can be hoisted by
LICM.  After the swap, the new "First" GEP has a *constant* offset; the
code tries to decide whether that constant lies within the underlying
object so it can re-apply the `inbounds` flag:

```cpp
APInt Offset(...);
Value *NewBase =
    First->stripAndAccumulateInBoundsConstantOffsets(DAL, Offset);
uint64_t ObjectSize;
if (!getObjectSize(NewBase, ObjectSize, DAL, TLI) ||
   Offset.ugt(ObjectSize)) {
  First->setNoWrapFlags(GEPNoWrapFlags::none());
  Second->setNoWrapFlags(GEPNoWrapFlags::none());
} else
  First->setIsInBounds(true);
```

There are two bugs glued together.

1. `stripAndAccumulateInBoundsConstantOffsets` only walks **inbounds**
   GEPs.  At this point `First` is **not** yet inbounds (the swap clears
   nowrap flags implicitly by overwriting one operand).  So the strip stops
   immediately, `NewBase = First`, and `Offset = 0`.
2. The check then asks `getObjectSize(First, ObjectSize, ...)` and tests
   `0 ugt ObjectSize`.  That comparison is false for any object whose size
   we can compute.  The pass therefore unconditionally calls
   `First->setIsInBounds(true)` whenever the static object size is known —
   **regardless of how large the constant offset on First actually is.**

The result is a `getelementptr inbounds` whose constant byte offset is
provably outside the underlying allocation.  Per LangRef, that GEP returns
`poison`, which downstream passes are then free to exploit.

## Reproducer

`/tmp/so_v3.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
@g = global [10 x i32] zeroinitializer        ; 40 bytes

define void @test(i64 %lim, i64 %step) {
entry:
  br label %loop
loop:
  %iv     = phi i64 [0, %entry], [%next, %loop]
  %addend = mul i64 %iv, %step
  %off    = add i64 %addend, 200              ; 200 i32 elements = 800 bytes
  %gep    = getelementptr i32, ptr @g, i64 %off  ; NOT inbounds
  store volatile i32 0, ptr %gep
  %next = add i64 %iv, 1
  %cmp  = icmp slt i64 %next, %lim
  br i1 %cmp, label %loop, label %exit
exit:
  ret void
}
```

Original IR: a non-inbounds GEP that may go anywhere.  The pointer is only
dereferenced when the loop runs at least one iteration, but the *address
computation* is always legal in the original program.

### After opt -passes='separate-const-offset-from-gep<lower-gep>'

```
loop:
  %iv     = phi i64 [ 0, %entry ], [ %next, %loop ]
  %addend = mul i64 %iv, %step
  %0      = shl i64 %addend, 2
  %uglygep  = getelementptr inbounds i8, ptr @g, i64 800   ; <-- LIE
  %uglygep2 = getelementptr i8, ptr %uglygep, i64 %0
  store volatile i32 0, ptr %uglygep2, align 4
  ...
```

`@g` is 40 bytes.  `getelementptr inbounds i8, ptr @g, i64 800` is
guaranteed-poison: 800 is not in `[0, 40]`.  Run instcombine on top and
it actively *strengthens* the flag set:

```
$ opt -passes='separate-const-offset-from-gep<lower-gep>,instcombine'
  %uglygep2 = getelementptr i8,
              ptr getelementptr inbounds nuw (i8, ptr @g, i64 800),
              i64 %0
```

Now `inbounds nuw` is asserted on an obviously-OOB constant-expr GEP.  This
is exactly the IR shape that GVN / CVP / NewGVN / ValueTracking rely on to
fold pointer comparisons, replace loads with `poison`, and prove branches
dead.  Any later pass that exploits the `inbounds` claim (e.g. a folder
deciding `cmp eq ptr %p, %q` cannot hold because `%p` is in `@g` and `%q`
is in a different object) will miscompile the program: the source program
allowed `%uglygep2` to alias other allocations as long as the loop didn't
execute (or the user wrapped the pointer back into range), but the
flag-strengthened IR forbids it.

## What's needed for a wrong-codegen demo from `llc`

The repro above produces unequivocally wrong IR.  To turn it into observed
wrong x86 assembly we just need to bait one of the inbounds-exploiting
folders.  GVN/CVP did not auto-trigger on `/tmp/so_v4.ll` with my exact
pipeline; a slightly larger program where `%gep` is compared to a pointer
in another object, or where the pointer is consumed by an SCEV-based
analysis (LSR, IndVarSimplify), should turn this into observable
miscompile.  The IR-level bug is independently a candidate.

## Where to fix

`llvm/lib/Transforms/Scalar/SeparateConstOffsetFromGEP.cpp:1508`

Either:

- After the swap, **accumulate the new (just-installed) constant operand of
  `First`** into `Offset` before comparing to `ObjectSize`, instead of
  relying on `stripAndAccumulateInBoundsConstantOffsets` which refuses to
  cross the not-yet-inbounds GEP, **or**
- only set `inbounds` if both pre-swap GEPs had `inbounds`/`nusw` and the
  new constant offset on `First`, plus the existing constant prefix on the
  base, demonstrably stays within the object.

The TODO comment at line 1525 (`Make flag preservation more precise`)
already acknowledges that this is hand-wavy; the test above shows that
"hand-wavy" is "wrong".

## Triage notes for parent

This is the smoking-gun version of "transform adds undefined-behavior flag
to a value that didn't have it".  It's a category that historically lands
front-and-center as soon as a downstream pass picks the IR up — see the
many GVN/CVP regressions over the years that traced back to overzealous
`inbounds`/`nsw`/`nuw` propagation.  Worth filing on the IR-correctness
ground alone, and a strong launching pad for a true end-to-end miscompile
once paired with the right downstream consumer.
