# ConstantFoldBinaryOpOperands silently drops nsw/nuw on Add/Sub/Xor ConstantExprs

## Summary

When InstCombine's per-instruction constant folder visits a constant
expression operand that is `add nsw`, `add nuw`, `add nuw nsw`, or
`sub nsw/nuw`, it rebuilds the ConstantExpr without its
`SubclassOptionalData` flags.  The freshly-built expression is then
substituted in.  The original (correct, flagged) ConstantExpr is dropped
on the floor.

This is observable: any constant expression like `add nsw (i64 ptrtoint
@A to i64, i64 ptrtoint @B to i64)` that is *not* further reducible
(because the symbolic operands prevent collapse) comes out of InstCombine
with no flags.  The result is identical numerically, but downstream
consumers that depended on `nsw`/`nuw` (range analysis, vectorization
profitability, IPO range/known-bits queries, etc.) lose information
unnecessarily.

Note: this is a missed-optimization / monotonic information loss, not a
miscompile.  Dropping `nsw`/`nuw` is sound (the result is the same value
plus a strictly weaker assumption).  But it does so even when no folding
was actually performed - the call merely rebuilds the same expression
without flags.

## Source

`llvm/lib/Analysis/ConstantFolding.cpp:1452-1463`
(`ConstantFoldBinaryOpOperands`):

```cpp
1452 Constant *llvm::ConstantFoldBinaryOpOperands(unsigned Opcode, Constant *LHS,
1453                                              Constant *RHS,
1454                                              const DataLayout &DL) {
1455   assert(Instruction::isBinaryOp(Opcode));
1456   if (isa<ConstantExpr>(LHS) || isa<ConstantExpr>(RHS))
1457     if (Constant *C = SymbolicallyEvaluateBinop(Opcode, LHS, RHS, DL))
1458       return C;
1459
1460   if (ConstantExpr::isDesirableBinOp(Opcode))
1461     return ConstantExpr::get(Opcode, LHS, RHS);     // <-- no flags
1462   return ConstantFoldBinaryInstruction(Opcode, LHS, RHS);
1463 }
```

The 4-argument `ConstantExpr::get(Opcode, LHS, RHS, Flags, ...)` is
available (see `llvm/lib/IR/Constants.cpp:2528`) and the constructor
already supports `SubclassOptionalData` for Add/Sub/Xor (see
`ConstantExpr::getAdd` / `getSub` at `Constants.cpp:2828` / `2835`).
The caller just doesn't pass them through.

The caller flow that reaches this path from InstCombine is:

```
InstCombiner::run -> ConstantFoldInstOperands(I, Ops, DL, TLI)
   -> ConstantFoldInstOperandsImpl(...)
       case isBinaryOp: ConstantFoldBinaryOpOperands(Op, LHS, RHS, DL)
           -> ConstantExpr::get(Op, LHS, RHS)        // drops flags
```

(See `llvm/lib/Analysis/ConstantFolding.cpp:1140-1158`.)

Also see the recursive `ConstantFoldConstantImpl`
(`ConstantFolding.cpp:1230`) which is what InstCombine uses to fold
constant operands of an instruction; it eventually reaches the same
flag-dropping path.

## Reproducer

`add_sub_nsw_drop.ll`:

```llvm
@A = external global i32
@B = external global i32

declare void @use(i64)

define void @add_nsw_used() {
  call void @use(i64 add nsw (i64 ptrtoint (ptr @A to i64),
                              i64 ptrtoint (ptr @B to i64)))
  ret void
}

define void @add_nuw_used() {
  call void @use(i64 add nuw (i64 ptrtoint (ptr @A to i64),
                              i64 ptrtoint (ptr @B to i64)))
  ret void
}

define void @add_nuw_nsw_used() {
  call void @use(i64 add nuw nsw (i64 ptrtoint (ptr @A to i64),
                                  i64 ptrtoint (ptr @B to i64)))
  ret void
}

define void @sub_nsw_used() {
  call void @use(i64 sub nsw (i64 ptrtoint (ptr @A to i64),
                              i64 ptrtoint (ptr @B to i64)))
  ret void
}
```

```
$ opt -S add_sub_nsw_drop.ll              # no passes: flags preserved
define void @add_nsw_used() {
  call void @use(i64 add nsw (i64 ptrtoint (ptr @A to i64), i64 ptrtoint (ptr @B to i64)))
  ret void
}
... (sub nsw and the others identical)

$ opt -passes=instcombine -S add_sub_nsw_drop.ll
define void @add_nsw_used() {
  call void @use(i64 add (i64 ptrtoint (ptr @A to i64), i64 ptrtoint (ptr @B to i64)))
  ret void                                ; <- nsw gone
}
define void @add_nuw_used() {
  call void @use(i64 add (i64 ptrtoint (ptr @A to i64), i64 ptrtoint (ptr @B to i64)))
  ret void                                ; <- nuw gone
}
define void @add_nuw_nsw_used() {
  call void @use(i64 add (i64 ptrtoint (ptr @A to i64), i64 ptrtoint (ptr @B to i64)))
  ret void                                ; <- nuw nsw gone
}
define void @sub_nsw_used() {
  call void @use(i64 sub (i64 ptrtoint (ptr @A to i64), i64 ptrtoint (ptr @B to i64)))
  ret void                                ; <- nsw gone
}
```

`-passes=instsimplify` and `-passes=early-cse` preserve the flags.
The bug is specific to the constant-folder path that
`ConstantFoldInstOperandsImpl` takes for ConstantExpr binops.

## Fix sketch

Pass `cast<ConstantExpr>(InstOrCE)->getRawSubclassOptionalData()` (or
similar) from `ConstantFoldInstOperandsImpl` into
`ConstantFoldBinaryOpOperands`, or thread the flags explicitly when the
operand under consideration is a binary `ConstantExpr`:

```cpp
unsigned Flags = 0;
if (auto *CE = dyn_cast<ConstantExpr>(InstOrCE))
  Flags = CE->getRawSubclassOptionalData();
...
return ConstantExpr::get(Opcode, LHS, RHS, Flags);
```

Note that `ConstantExpr::get` already accepts a `Flags` parameter
(`Constants.cpp:2528-2567`); the wiring is the only thing missing.

Alternatively, recognize that `LHS`/`RHS` are unchanged (no actual
folding happened) and return `const_cast<ConstantExpr*>(this)` instead
of rebuilding it.

## Why this matters at -O2

- The `ptrtoint` / `add nsw` / `sub nsw` idiom appears in real C++ code
  (e.g., `(intptr_t)&A - (intptr_t)&B`, vtable layout calculations,
  `offsetof`-style expressions across different objects).
- `nsw`/`nuw` is the primary signal LLVM range analysis and IR-level
  vectorizer profitability use. Silently stripping them in InstCombine
  means later passes (LSR, IndVars, LV, GVN) see less information than
  the IR contained at parse time.
- Even when no fold happens, the rewrite is unconditional, so every
  `-O2` run pays the cost on every such expression.
