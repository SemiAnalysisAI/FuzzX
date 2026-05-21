# w102: LoopUnroll runtime (prolog/epilog) with volatile load — correctness check

## Status

Empirical observation: **NOT a miscompile** — runtime unroll correctly preserves volatile load count for prolog and epilog variants.

## Pattern

Runtime loop unrolling splits an unknown-trip-count loop into a prolog (or epilog) handling the remainder iterations plus a fully-unrolled main loop. A volatile load must execute **exactly N times** for trip count N. Any optimization that elides one of the volatile loads in the unrolled body or moves it across the prolog boundary would be a miscompile.

## Repro

File: `/tmp/w102/t5_runtime_vol.ll`

```llvm
define i32 @rt_vol(ptr %p, i32 %n) {
entry: br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.n, %loop ]
  %acc = phi i32 [ 0, %entry ], [ %acc.n, %loop ]
  %v = load volatile i32, ptr %p, align 4
  %acc.n = add i32 %acc, %v
  %i.n = add i32 %i, 1
  %c = icmp ult i32 %i.n, %n
  br i1 %c, label %loop, label %exit
exit: ret i32 %acc.n
}
```

## opt diff (epilog) excerpt

```
opt -S -passes='loop-unroll' -unroll-runtime -unroll-runtime-epilog=true -unroll-count=4
```

Main loop body has 4 volatile loads (`%v`, `%v.1`, `%v.2`, `%v.3`); epilog has 1 volatile load. Total trip count is exactly `n` (matches original). Same for prolog variant.

```llvm
loop:                                             ; the unrolled main loop
  %v   = load volatile i32, ptr %p, align 4
  %v.1 = load volatile i32, ptr %p, align 4
  %v.2 = load volatile i32, ptr %p, align 4
  %v.3 = load volatile i32, ptr %p, align 4
  ...
loop.epil:
  %v.epil = load volatile i32, ptr %p, align 4
```

## Verdict

Volatile is correctly preserved on every copy in both prolog (-unroll-runtime-epilog=false) and epilog (-unroll-runtime-epilog=true) lowering. Loop trip count via `xtraiter = and %n, 3` plus main `unroll_iter` correctly sums to `%n`. `loadCSE` in `simplifyLoopAfterUnroll` rejects via `Load->isSimple()` check (LoopUnroll.cpp:306).

No miscompile observed.
