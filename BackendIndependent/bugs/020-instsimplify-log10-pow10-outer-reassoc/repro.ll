declare double @llvm.pow.f64(double, double)
declare double @llvm.log10.f64(double)

define double @log10_pow10(double %x) {
  %p = call double @llvm.pow.f64(double 1.000000e+01, double %x)
  %r = call reassoc double @llvm.log10.f64(double %p)
  ret double %r
}
