declare double @llvm.pow.f64(double, double)

define double @pow_same_exponent(double %x, double %z, double %y) {
  %p0 = call double @llvm.pow.f64(double %x, double %y)
  %p1 = call double @llvm.pow.f64(double %z, double %y)
  %r = fmul reassoc double %p0, %p1
  ret double %r
}
