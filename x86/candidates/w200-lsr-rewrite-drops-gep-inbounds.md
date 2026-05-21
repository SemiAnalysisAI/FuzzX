## LoopStrengthReduce: SCEV-expanded GEP drops original GEP's `inbounds`

**Severity:** Missed optimization (not a miscompile by itself, but the lost
`inbounds` flag is load-bearing for downstream alias analysis, vectorization,
and SROA).

**File:** `llvm/lib/Transforms/Utils/ScalarEvolutionExpander.cpp:386-437`
(`SCEVExpander::expandAddToGEP`) — invoked by LSR via
`llvm/lib/Transforms/Scalar/LoopStrengthReduce.cpp:5834`.

### What goes wrong

When LSR (`-passes='loop-mssa(loop-reduce)'`) rewrites a GEP-based address
into the canonical IV form, it routes the expansion through
`SCEVExpander::expandAddToGEP`. The SCEV-expanded GEP is built with:

```cpp
GEPNoWrapFlags NW = any(Flags & SCEV::FlagNUW)
                        ? GEPNoWrapFlags::noUnsignedWrap()
                        : GEPNoWrapFlags::none();
...
return Builder.CreatePtrAdd(V, Idx, "scevgep", NW);
```

SCEV only tracks `FlagNUW`/`FlagNSW`, so the GEP's `inbounds` (and `nusw`)
bit is *never* set when expanding pointer arithmetic. There is no
intersection-with-original-flags step here: even when LSR is rewriting a
GEP that was `getelementptr inbounds ...` in the source IR, the
replacement is plain `getelementptr i8, ...`.

### Repro

```ll
; reducer.ll
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @use(ptr) #0
attributes #0 = { nounwind }

define void @test_lsr_complex(ptr noalias %p, i64 %n) {
entry:
  br label %loop

loop:
  %i  = phi i64 [ 0, %entry ], [ %i.next, %loop ]
  %off = mul i64 %i, 8
  %p1 = getelementptr inbounds i8, ptr %p, i64 %off
  call void @use(ptr %p1)
  %i.next = add nuw nsw i64 %i, 1
  %cmp    = icmp ult i64 %i.next, %n
  br i1 %cmp, label %loop, label %exit

exit:
  ret void
}
```

`opt -passes='loop-mssa(loop-reduce)' -S reducer.ll`:

```ll
loop:
  %lsr.iv  = phi ptr [ %scevgep, %loop ], [ %p, %entry ]
  %i       = phi i64 [ 0, %entry ], [ %i.next, %loop ]
  call void @use(ptr %lsr.iv)
  %i.next  = add nuw i64 %i, 1
  %scevgep = getelementptr i8, ptr %lsr.iv, i64 8   ; <-- inbounds dropped
  %cmp     = icmp ult i64 %i.next, %n
  br i1 %cmp, label %loop, label %exit
```

Original GEP: `getelementptr inbounds i8, ptr %p, i64 %off`.
After LSR:     `getelementptr i8, ptr %lsr.iv, i64 8` — **no `inbounds`,
no `nusw`**, even though the address sequence is provably inbounds
(LSR is just stepping the same pointer by a constant).

### Why this matters in the -O2 pipeline

`inbounds` is not just a hint — downstream passes use it to prove no
aliasing across distinct base objects, to enable vectorization, and to
allow speculation. The default `-O2` pipeline runs LSR fairly late, and
some downstream loop / alias passes (LoopVectorize, LoopVersioning,
LICM) consult GEP `inbounds`. Dropping `inbounds` blanket-wise from
LSR's rewrites is a known regression vector — particularly for loops
where the user-visible pointer math uses `inbounds` GEPs that LSR
collapses into stride-form.

### Source-level evidence

- `ScalarEvolutionExpander.cpp:392-394` — only translates `FlagNUW`,
  never `inbounds`/`nusw`:

  ```cpp
  GEPNoWrapFlags NW = any(Flags & SCEV::FlagNUW)
                          ? GEPNoWrapFlags::noUnsignedWrap()
                          : GEPNoWrapFlags::none();
  ```

- `ScalarEvolutionExpander.cpp:413-414` — when reusing a nearby GEP, it
  *intersects* (only ever weakens), but the freshly created GEP at
  line 436 starts from `NW` only.

- LSR's `LSRInstance::Rewrite` does not communicate any per-fixup
  `inbounds` requirement to the expander. Even though the original IR
  the user wrote was `getelementptr inbounds`, LSR never consults it.

### Suggested fix

In `SCEVExpander::expandAddToGEP`, also OR-in `inbounds` if SCEV's
range analysis can prove the pointer arithmetic stays within the
allocated object (which for unit-stride IVs over a known trip count is
trivially true). Alternatively, LSR's `LSRInstance::Rewrite` should
peek at the original `LF.UserInst`'s GEP operand and propagate its
`inbounds` to the expander's hint.

### Status

Confirmed via `opt -passes='loop-mssa(loop-reduce)' -S` diff (LSR alone
strips `inbounds`). At full `-O2`, LSR runs late and downstream passes
like InstCombine may re-add some inbounds via reasoning of their own,
but loops where LSR fires and downstream cannot recover keep the
weakened GEP. Not a miscompile by itself (dropping `inbounds` is a
weakening, hence safe), but a documented missed-optimization hazard
in the SCEV-based pipeline.
