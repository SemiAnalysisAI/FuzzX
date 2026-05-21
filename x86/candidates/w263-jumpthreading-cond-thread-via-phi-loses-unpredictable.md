# w263: JumpThreading threading a cond branch on a PHI loses `!unpredictable` from the surviving conditional branch

## Pass
`-passes=jump-threading` (default x86 -O2 pipeline includes this).

## Summary

When JumpThreading folds the diamond pattern

```
entry: br i1 %c, P1, P2
P1:    br BB
P2:    br BB
BB:    %phi = phi i1 [true,P1],[false,P2]
       ...
       br i1 %phi, T, F, !prof !W, !unpredictable !{}
```

into a direct `br i1 %c, T, F` on `entry`, the resulting conditional
branch carries the rewritten `!prof` but **never `!unpredictable`**.

`grep MD_unpredictable JumpThreading.cpp` returns zero hits — the pass
has no logic anywhere to transfer `!unpredictable` to a synthesized
branch. The terminator-cloning path in `duplicateCondBranchOnPHIIntoPred`
(at `JumpThreading.cpp:2695`) does preserve it via `BI->clone()`, but
subsequent constant-fold of `br i1 const, ...` to `br uncond` discards
it. Either the original `!unpredictable` should be forwarded to the
surviving conditional branch (the one in `entry`), or
`ConstantFoldTerminator` should be taught to propagate it — but right
now JT, which is the one introducing the situation where the original
`!unpredictable` becomes unreachable, doesn't do either.

## Reproducer

Input `final_d.ll`:
```llvm
target triple = "x86_64-unknown-linux-gnu"
declare i32 @callee()

define i32 @test_dup_phi_br(i1 %c, ptr %p) {
entry:
  br i1 %c, label %P1, label %P2
P1:
  br label %BB
P2:
  br label %BB
BB:
  %phi = phi i1 [ true, %P1 ], [ false, %P2 ]
  %v = load i32, ptr %p
  store i32 %v, ptr %p
  br i1 %phi, label %T, label %F, !prof !1, !unpredictable !2
T:
  ret i32 1
F:
  ret i32 2
}
!1 = !{!"branch_weights", i32 99, i32 1}
!2 = !{}
```

```
opt -passes=jump-threading -S final_d.ll
```

Output:
```llvm
entry:
  br i1 %c, label %T, label %F, !prof !0         ; <-- no !unpredictable
T:
  %v2 = load i32, ptr %p, align 4
  store i32 %v2, ptr %p, align 4
  ret i32 1
F:
  %v = load i32, ptr %p, align 4
  store i32 %v, ptr %p, align 4
  ret i32 2
}
!0 = !{!"branch_weights", i32 2126008812, i32 21474836}
```

Notice `!prof` survives (transformed weights are even
recomputed from the original). `!unpredictable !2` from the *only*
conditional branch in the input is gone.

## Source locations

- `JumpThreading.cpp:2695` — `Instruction *New = BI->clone();` in
  `duplicateCondBranchOnPHIIntoPred`. clone() preserves
  `!unpredictable` on the cloned conditional branch, but the cloned
  branch has a constant condition and is subsequently folded.
- `JumpThreading.cpp:2619` — `setBranchWeights(*TI, Weights, ...)` in
  `updateBlockFreqAndEdgeWeight`. Only handles `MD_prof`.
- `JumpThreading.cpp` (whole file) — no `MD_unpredictable` references.

## Why this matters

`!unpredictable` is a load-bearing CodeGen hint: it tells the back end
not to convert the branch into a `cmov`. The transformation is
semantics-preserving only if the surviving conditional branch carries
the same `!unpredictable` annotation that the user attached to the
fused branch. Otherwise the generated x86 code can pick a `cmov`
strategy that the user explicitly opted out of via
`__builtin_unpredictable`.

## Suggested fix

In `duplicateCondBranchOnPHIIntoPred`, when the cloned terminator is
about to be constant-folded by simplifyInstruction, propagate
`MD_unpredictable` from the original cond branch to the predecessor's
*new* unconditional/conditional branch. Equivalently, teach
`ConstantFoldTerminator` to drop `MD_unpredictable` on the original
branch *before* replacing it with `br uncond` so that any caller that
clones the original first picks it up correctly. The single-line fix
that catches the JT path is, in the BB whose successors got rewired to
`T`/`F`, copy `MD_unpredictable` from the original `BI` onto the new
terminator there.
