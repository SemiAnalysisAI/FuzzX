declare double @llvm.log10.f64(double)
declare double @llvm.exp10.f64(double)

define double @exp10_log10(double %x) {
  %l = call double @llvm.log10.f64(double %x)
  %r = call reassoc double @llvm.exp10.f64(double %l)
  ret double %r
}
