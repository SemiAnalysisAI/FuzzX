declare double @llvm.exp.f64(double)

define double @exp_product(double %x, double %y) {
  %e0 = call double @llvm.exp.f64(double %x)
  %e1 = call double @llvm.exp.f64(double %y)
  %r = fmul reassoc double %e0, %e1
  ret double %r
}
