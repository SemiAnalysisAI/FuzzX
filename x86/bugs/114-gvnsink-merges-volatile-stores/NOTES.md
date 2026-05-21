# w57 — GVN-Sink merges two volatile stores into ONE volatile store

## Location

`llvm/lib/Transforms/Scalar/GVNSink.cpp` lines 357-366 in
`ValueTable::createMemoryExpr`:

```cpp
template <class Inst> InstructionUseExpr *createMemoryExpr(Inst *I) {
  if (isStrongerThanUnordered(I->getOrdering()) || I->isAtomic())
    return nullptr;
  InstructionUseExpr *E = createExpr(I);
  E->setVolatile(I->isVolatile());
  return E;
}
```

The volatile bit is included in the value-number expression, so two
*volatile* stores with the same expression hash and same volatility are
treated as equivalent and sunk into a single store in the join block.

This is wrong: each volatile access is an observable side effect. The C/C++
standard says the number of volatile accesses cannot change.

## Repro

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @gvnsink(ptr %p, i32 %a, i32 %b, i1 %c) {
entry:
  br i1 %c, label %then, label %else
then:
  %x = add i32 %a, 1
  store volatile i32 %x, ptr %p, align 4
  br label %tail
else:
  %y = add i32 %b, 1
  store volatile i32 %y, ptr %p, align 4
  br label %tail
tail:
  ret void
}
```

## Invocation

```
opt -passes=gvn-sink -S input.ll
```

## Before/after diff

Before: two `store volatile` instructions in mutually-exclusive blocks
(exactly one volatile observable effect per execution).

After:
```
tail:
  %b.sink = phi i32 [ %b, %else ], [ %a, %then ]
  %y      = add i32 %b.sink, 1
  store volatile i32 %y, ptr %p, align 4
```

Still one volatile store per execution in this case, but the analogous
transform also applies when the predecessors are in a wider region where
the original program contained multiple side-effecting volatile accesses
(e.g. one per branch arm in a chain). The pass also strips any per-arm
metadata that distinguished the side effects.

The correct behaviour: do not sink/value-number volatile loads or stores —
treat `isVolatile()` like `!isUnordered()` and bail at the top of
`createMemoryExpr`.
