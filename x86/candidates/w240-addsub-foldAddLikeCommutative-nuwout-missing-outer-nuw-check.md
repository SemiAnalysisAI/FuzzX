# w240: foldAddLikeCommutative NUWOut for (A-B)+(C-A) -> C-B ignores outer NUW flag

## File / Region
- `llvm/lib/Transforms/InstCombine/InstCombineAddSub.cpp`, function
  `InstCombinerImpl::foldAddLikeCommutative`, lines ~1328-1342.

## Code
```cpp
Instruction *InstCombinerImpl::foldAddLikeCommutative(Value *LHS, Value *RHS,
                                                      bool NSW, bool NUW) {
  Value *A, *B, *C;
  if (match(LHS, m_Sub(m_Value(A), m_Value(B))) &&
      match(RHS, m_Sub(m_Value(C), m_Specific(A)))) {
    Instruction *R = BinaryOperator::CreateSub(C, B);
    bool NSWOut = NSW && match(LHS, m_NSWSub(m_Value(), m_Value())) &&
                  match(RHS, m_NSWSub(m_Value(), m_Value()));

    bool NUWOut = match(LHS, m_NUWSub(m_Value(), m_Value())) &&
                  match(RHS, m_NUWSub(m_Value(), m_Value()));
    R->setHasNoSignedWrap(NSWOut);
    R->setHasNoUnsignedWrap(NUWOut);
    return R;
  }
  ...
}
```

## Observation
`NSWOut` correctly conjoins the outer-add NSW flag (`NSW &&` prefix), but `NUWOut`
does NOT include the outer-add NUW flag. The fold sets `nuw` on the resulting
`(C - B)` purely from the sub operands.

## Analysis (Alive2-style)
For `(A -nuw B) + (C -nuw A) -> C - B`:
- `A -nuw B` nuw: A >= B (unsigned).
- `C -nuw A` nuw: C >= A (unsigned).
- Transitively C >= A >= B, so `C - B` does NOT underflow unsigned.

So the implication holds INDEPENDENTLY of the outer-add NUW flag.
Setting NUW on the result based on operand flags only is **mathematically
correct** — no bug. The asymmetry with NSW handling is intentional: NSW
requires the outer add nsw because two individually-nsw subs can still sum
to overflow.

## Reproducer
Source: `/tmp/w240/t21_addsub_fold.ll`

```llvm
define i32 @addsub_fold(i32 %a, i32 %b, i32 %c) {
  %s1 = sub nuw i32 %a, %b
  %s2 = sub nuw i32 %c, %a
  %r = add i32 %s1, %s2    ; no flags on outer add
  ret i32 %r
}
```

`opt -passes=instcombine -S` output:
```llvm
define i32 @addsub_fold(i32 %a, i32 %b, i32 %c) {
  %r = sub nuw i32 %c, %b
  ret i32 %r
}
```

## Verdict
**NOT a miscompile.** The asymmetric NSW vs NUW logic is intentional and
mathematically sound. Documented here for completeness; readers exploring
this fold may wonder about the asymmetry.
