# processUGT_ADDCST_ADD bails when CI1->getBitWidth() == NewWidth (missed fold, harmless)

File: `llvm/lib/Transforms/InstCombine/InstCombineCompares.cpp` lines 1089-1092

```cpp
// Check to see that CI1 is an all-ones value with NewWidth bits.
if (CI1->getBitWidth() == NewWidth ||
    CI1->getValue() != APInt::getLowBitsSet(CI1->getBitWidth(), NewWidth))
  return nullptr;
```

## Reasoning

The first clause `CI1->getBitWidth() == NewWidth` bails out unconditionally
when the icmp's RHS constant has the same bit-width as the candidate
narrow add. But the second clause is the actual semantic check: CI1 must
be an `(1<<NewWidth)-1` all-ones value. When `BitWidth == NewWidth`,
`APInt::getLowBitsSet(NewWidth, NewWidth)` produces `(1<<NewWidth)-1`
which is the entire NewWidth value, i.e. `CI1->getValue()` must equal
that.

In other words, the `||` should arguably be `&&` here, or the first
clause should be `CI1->getBitWidth() < NewWidth`. As written, the
pattern
```
%sum = add (add A, B), 128
%cmp = icmp ugt i8 %sum, 127     ; CI2=128 i8, CI1=127 i8 — same width
```
is rejected even though the values fit and the underlying transformation
to `sadd_with_overflow.i8` would be semantically correct.

This is a missed-optimization, not a miscompile — but the typo-shaped
`==` (instead of likely-intended `<` or absent) is fragile: a future
refactor that changes the bail logic could turn it into a real
miscompile by inverting the check.

## Repro

```
; opt -passes=instcombine -S
define i1 @f(i8 %a, i8 %b) {
  %ext.a = sext i8 %a to i8
  %ext.b = sext i8 %b to i8
  %add = add i8 %ext.a, %ext.b
  %sum = add i8 %add, 128
  %cmp = icmp ugt i8 %sum, 127
  ret i1 %cmp
}
```
Expected: `sadd.with.overflow.i8(%a, %b)` extraction. Actual: pattern not folded.

## Expected wrong outcome

Missed optimization only. Filing as a fragile-guard / refactor-risk
note rather than a confirmed miscompile.
