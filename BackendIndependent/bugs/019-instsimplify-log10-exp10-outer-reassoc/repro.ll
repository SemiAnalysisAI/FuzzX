declare double @llvm.exp10.f64(double)
declare double @llvm.log10.f64(double)

define double @log10_exp10(double %x) {
  %e = call double @llvm.exp10.f64(double %x)
  %r = call reassoc double @llvm.log10.f64(double %e)
  ret double %r
}
