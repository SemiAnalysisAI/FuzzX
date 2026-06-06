declare double @llvm.pow.f64(double, double)

define double @pow_same_base(double %x, double %y, double %z) {
  %p0 = call double @llvm.pow.f64(double %x, double %y)
  %p1 = call double @llvm.pow.f64(double %x, double %z)
  %r = fmul reassoc double %p0, %p1
  ret double %r
}
