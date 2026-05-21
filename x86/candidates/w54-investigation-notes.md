# worker-54 investigation notes (2026-05-21)

No confirmed miscompiles in ~10 minute window. Patterns investigated:

## 1. fshl/fshr edge cases
- `fshl.i32(a, b, 32)` -> folded to `%a` (correct, shift mod bw)
- `fshl.i32(a, b, 33)` -> folded to `fshl(_, _, 1)` (correct)
- `fshl.i7(a, b, -1)` -> folded to `fshl(_, _, 1)` since `-1 mod 7 = 1` (correct)
- Rotate idiom recognized correctly; lshr-by-32 UB cleanly refined to fshl

## 2. umul.with.overflow / smul.with.overflow folds
- `umul.with.overflow.i32(a, -1)` -> `icmp ugt a, 1` (correct, since 0*X=0, 1*X=X, 2*X overflows)
- `smul.with.overflow.i32(a, -1)` -> `icmp eq a, INT_MIN` (correct - only INT_MIN * -1 overflows)
- Vector v4i32 lowering looked complex but no obvious bug

## 3. Saturating arithmetic
- `usub.sat(x, all-ones)` -> 0 (correct: x - max-uint always saturates to 0)
- `uadd.sat.v8i16(a, splat -1)` -> `splat -1` (correct)
- `sadd.sat(127,1)` -> 127 (correct)

## 4. smin/smax/abs chain folds
- `smin(smax(a,5),3)` -> 3 (correct via interval analysis: smax(a,5)>=5 > 3)
- `smax(a, INT_MIN)` -> a (trivial)
- `smin(a, INT_MAX)` -> a (trivial)

## 5. vpermilvar selector mask
- `vpermilvar.ps` with selectors {7,-1,5,6} -> `vshufps $159` = lanes [3,3,1,2]
- Correct because vpermilvar.ps uses only bits[1:0]; 7&3=3, -1&3=3, 5&3=1, 6&3=2

## 6. Vector reductions
- `vector.reduce.or.v4i1(<false,false,true,false>)` -> true (correct)

## 7. ext-folds
- `sext(lshr x, 7)` -> `zext nneg` (correct, top bit of result is 0)
- `sext(i1 x) & 1` -> `zext(i1 x)` (correct, low bit of {0,-1} is x itself)

## 8. ctlz/cttz
- `ctlz(0, true)` -> poison (correct per is_zero_undef semantics)
- `cttz(x, true)` + select-on-zero(x,32,t) -> `cttz(x, false)` (correct: select makes x=0 case safe)

## 9. vpternlog (unrecognized intrinsic, calls extern - skipped)

## 10. smul.fix.sat.i32 lowering looked complex but did not confirm wrong

All folds investigated were correct per LangRef semantics. No reproducible
miscompile found in this window.
