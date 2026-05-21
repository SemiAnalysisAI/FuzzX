# NewGVN CSEs distinct `freeze` instructions with same input — violates LangRef

## File and root cause

`llvm/lib/Transforms/Scalar/NewGVN.cpp:2022` — `performSymbolicEvaluation`
falls through `Freeze` to `createExpression(I)`, which builds a `BasicExpression`
keyed only on `(opcode, type, operand-leaders)`. Two `freeze` instructions with
the same single operand therefore produce identical expressions and are unified
into one CongruenceClass.

```
case Instruction::AddrSpaceCast:
case Instruction::Freeze:
  return createExpression(I);
```

`BasicExpression::equals` (`GVNExpression.h:213`) compares opcode + type +
operand pointers only. There is no "fresh choice" / unique-id concept for
freeze.

## Why this is unsound

LangRef on `freeze`:

> If the input is undef or poison, freeze returns an arbitrary, but fixed,
> value of type ty. Otherwise, this instruction is a no-op and returns the
> input value.

Crucially, **each `freeze` instruction picks its own arbitrary value**; the
choice is not shared across instances. Two `freeze i32 undef` may legally
return 0 and 1 respectively. So they must NOT be CSE'd.

NewGVN doing so allows the program to deduce false implications between the
two values, eliminating control flow paths that the source language permits.

## IR repro (poison/undef input — clearest case)

```llvm
define i32 @test() {
entry:
  %x = freeze i32 undef
  %y = freeze i32 undef
  %c1 = icmp eq i32 %x, 0
  %c2 = icmp eq i32 %y, 1
  %and = and i1 %c1, %c2
  %r = select i1 %and, i32 42, i32 0
  ret i32 %r
}
```

Source semantics: `%x` may freeze to 0 and `%y` to 1 — independent choices —
so the program may legitimately return 42.

## opt diff

```
$ opt -passes=newgvn -S
define i32 @test() {
entry:
  %x = freeze i32 undef
  %c1 = icmp eq i32 %x, 0
  %c2 = icmp eq i32 %x, 1     ; <-- %y RAUW'd to %x
  %and = and i1 %c1, %c2      ; (x==0) && (x==1) is unsatisfiable
  %r = select i1 %and, i32 42, i32 0
  ret i32 %r
}

$ opt -passes='newgvn,instcombine' -S
define i32 @test() {
entry:
  ret i32 0                   ; <-- 42 path eliminated
}
```

The original program's 42-returning execution is gone.

## llc diff (x86_64-linux-gnu)

```
$ llc <newgvn output>
test:
    xorl %eax, %eax           ; ret 0 always
    retq
```

Without NewGVN the freeze divergence is preserved and the path to 42 stays in
the program (subsequent passes may still be unable to fold but at least the
semantic possibility remains).

## Same-operand non-undef variant

```llvm
define i32 @test(i32 %a) {
  %fa = freeze i32 %a
  %fb = freeze i32 %a
  %r = sub i32 %fa, %fb
  ret i32 %r
}
```

NewGVN folds the function to `ret i32 0`. This is sound IF `%a` is never
poison, but `%a` is an arbitrary parameter; if the caller passes a poison
value, both `%fa` and `%fb` independently choose, so the difference is not
necessarily 0.

## Suggested fix

`freeze` must not participate in normal value numbering. Either:

* Give each `freeze` its own unique `UnknownExpression` (so it is its own
  congruence class), OR
* Only CSE two `freeze` instructions when the input is known to never be
  poison (use `isGuaranteedNotToBePoison`), which preserves the no-op case.

Note: legacy GVN has the same misbehavior; the freeze-as-fresh-choice
contract is widely under-implemented across the optimizer, but NewGVN should
be filed independently because the fix lives in this file.
