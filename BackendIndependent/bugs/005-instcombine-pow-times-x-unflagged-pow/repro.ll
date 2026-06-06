declare double @llvm.pow.f64(double, double)

define double @pow_mul_x(double %x, double %y) {
  %p = call double @llvm.pow.f64(double %x, double %y)
  %r = fmul reassoc double %p, %x
  ret double %r
}
