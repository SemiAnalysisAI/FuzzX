# w102: LoopRotate duplicates atomic-acquire load into preheader (semantic-equivalence question)

## Status

Empirical observation: **NOT a miscompile** under current LLVM semantics, but flagged for review.

## Pattern

LoopRotate hoists/duplicates the loop header into the preheader. When the header contains an `atomic acquire` load (or `seq_cst`, `release`, etc.), the load is also placed in the preheader. In the original IR, the load must execute at least once if the function reaches the header (i.e., always when reaching the loop region, since `head` is the entry of the loop). After rotation, the load fires exactly once in the preheader plus once per executed body iteration — semantically equivalent in terms of execution count.

However, the **atomic ordering edge** moves: the acquire load in the preheader is now ordered relative to any preceding code in the preheader rather than relative to the loop body's PHI dominators. For pure SSA this is fine, but for memory-model reasoning the synchronization-with edge is now anchored to the preheader.

## Repro

File: `/tmp/w102/t12_rot_atomic_align.ll`

```llvm
define i32 @rot_atom(ptr %p, i32 %n) {
entry:
  br label %head
head:
  %i = phi i32 [ 0, %entry ], [ %i.n, %body ]
  %acc = phi i32 [ 0, %entry ], [ %acc.n, %body ]
  %v = load atomic i32, ptr %p syncscope("singlethread") acquire, align 16
  %c = icmp slt i32 %i, %n
  br i1 %c, label %body, label %exit
body:
  %acc.n = add i32 %acc, %v
  %i.n = add i32 %i, 1
  br label %head
exit:
  %r = phi i32 [ %acc, %head ]
  ret i32 %r
}
```

## opt diff (excerpt)

After `opt -passes='loop-rotate'`:

```llvm
entry:
  %v1 = load atomic i32, ptr %p syncscope("singlethread") acquire, align 16   ; <- NEW
  %c2 = icmp slt i32 0, %n
  br i1 %c2, label %body.lr.ph, label %exit
...
body:
  %v5 = phi i32 [ %v1, %body.lr.ph ], [ %v, %body ]   ; uses preheader-load
  ...
  %v = load atomic i32, ptr %p syncscope("singlethread") acquire, align 16
```

## Verdict

The atomic ordering, syncscope, and alignment are correctly preserved on both copies of the load. Per LangRef, the original IR's header executes the load whenever control flows from `entry` to `head`, so the load always fires at least once. LoopRotate preserves this — but the load in the preheader is no longer "inside the loop iteration" textually. Not a miscompile under LangRef; flagged for memory-model auditing.
