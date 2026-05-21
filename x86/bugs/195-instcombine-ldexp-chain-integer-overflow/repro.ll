; Two chained ldexp with INT_MAX exponents.
; Mathematical result: ldexp(x, INT_MAX + INT_MAX) saturates to +inf (for positive x).
; InstCombine folds the chain to ldexp(x, INT_MAX+INT_MAX) where the i32 sum wraps to -2,
; producing fmul x, 0.25 — the WRONG sign of overflow.
declare double @llvm.ldexp.f64.i32(double, i32)
define double @repro(double %x) {
  %a1 = and i32 2147483647, 2147483647   ; INT_MAX
  %a2 = and i32 2147483647, 2147483647   ; INT_MAX
  %r1 = call double @llvm.ldexp.f64.i32(double %x, i32 %a1)
  %r2 = call double @llvm.ldexp.f64.i32(double %r1, i32 %a2)
  ret double %r2
}
