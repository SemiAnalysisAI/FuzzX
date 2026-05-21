# w110 Standard GVN CSEs distinct `freeze` instructions of same operand (mirrors w77 NewGVN bug)

## Location

`llvm/lib/Transforms/Scalar/GVN.cpp:704` — `ValueTable::lookupOrAddImpl`
unconditionally falls through `Instruction::Freeze` to `createExpr(I)`:

```
case Instruction::Freeze:
  Exp = createExpr(I);
  break;
```

`createExpr` keys the expression by `(opcode, type, operand value-numbers)`
with no per-instruction unique tag. Two distinct `freeze` instructions whose
single operand has the same VN are therefore assigned the same value number
and one replaces the other.

This is the **standard GVN** analogue of the NewGVN issue captured in
`w77-newgvn-freeze-cse-same-operand-not-fresh-choice.md`. The freeze-handling
flaw exists independently in both GVN passes.

## Why this is unsound

LangRef on `freeze`:

> If the input is undef or poison, freeze returns an arbitrary, but fixed,
> value of type ty.

Each distinct `freeze` instruction makes its own arbitrary choice. Two
freezes of the same poison operand may legally return **different** non-poison
values; merging them forces them to return the same value, which is a
miscompile in the worst case (subtract → 0 instead of nonzero) and a
soundness loss for downstream analyses in any case.

## Reproducer

`/tmp/w110-tests/test_freeze_clean.ll`:

```ll
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(i32 %x) {
entry:
  %s = shl i32 1, %x       ; poison when %x >= 32
  %f1 = freeze i32 %s
  %f2 = freeze i32 %s
  %d = sub i32 %f1, %f2
  ret i32 %d
}
```

## opt diff (standard GVN, `-passes=gvn`)

```
$ build/llvm-fuzzer/bin/opt -S -passes=gvn test_freeze_clean.ll
define i32 @test(i32 %x) {
entry:
  %s = shl i32 1, %x
  %f1 = freeze i32 %s
  ret i32 0                ; <-- WRONG: assumes f1 == f2
}
```

Original IR with `%x = 32`: `%s` is poison; `%f1` may pick e.g. `0x1234`,
`%f2` may pick `0x5678`, `%d = 0x1234 - 0x5678 ≠ 0`. GVN replaces with
constant `0`, eliminating the second freeze entirely.

## x86 backend visibility

`llc` lowers `ret i32 0` to a single `xor eax, eax` whereas the unoptimized
form lowers `freeze` to no-ops around `shlxl` + `subl`. So the miscompile is
observable in the final emitted machine code (not source-only).

## Suggested fix

Match NewGVN's intended (but also buggy there) behavior — and instead of
`createExpr(I)`, either:

  1. Treat `freeze` as opaque (assign a fresh VN per instance) — same approach
     as `call` with side effects.
  2. Add a per-instruction discriminator to the expression so two freezes
     of the same operand do not collide.

The cleanest local fix is to bail out before `createExpr`:

```c++
case Instruction::Freeze:
  Exp.opcode = ~0u;             // never-equal sentinel
  ValueNumbering[V] = NextValueNumber;
  return NextValueNumber++;
```

## Severity

Real Alive2-falsifiable miscompile. Status disputed historically because
some passes already assume freeze is single-valued, but per the LangRef
text quoted above the optimization is unsound and worth being explicit
about in standard GVN to match NewGVN behavior.
