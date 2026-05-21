# 194 — InstCombine alone CSEs distinct `freeze` instructions of same poison-able operand

Component: InstCombine (default O2)

Same family as #187 (Standard GVN), #188 (EarlyCSE), #136 (NewGVN), but in **InstCombine** itself. InstCombine recognizes `freeze X; freeze X` as redundant when X is a single SSA value, even when X may produce poison — folds `f1 - f2 → 0`.

## Reproducer

```ll
define i32 @f(i32 %x) {
  %s = shl i32 1, %x       ; poison when x >= 32
  %f1 = freeze i32 %s
  %f2 = freeze i32 %s
  %d = sub i32 %f1, %f2
  ret i32 %d
}
```

`opt -passes=instcombine -S` → `ret i32 0`.

Per LangRef each freeze independently picks an arbitrary fixed value when its input is poison/undef. With `%x = 32`, `%s` is poison; `%f1` may pick e.g. `0x1234`, `%f2` may pick `0x5678`; `%d = -0x4444 ≠ 0`. InstCombine claims they're identical.

InstCombine is **the** workhorse of `-O2`; this bug fires unconditionally in the standard pipeline.

## Severity

Alive2-falsifiable. Affects code that uses `freeze` to safely consume a possibly-poison value multiple times (common idiom for promoting `select i1 poison` etc.).
