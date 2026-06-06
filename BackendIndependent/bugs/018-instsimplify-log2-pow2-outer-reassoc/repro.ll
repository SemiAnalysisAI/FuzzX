declare double @llvm.pow.f64(double, double)
declare double @llvm.log2.f64(double)

define double @log2_pow2(double %x) {
  %p = call double @llvm.pow.f64(double 2.000000e+00, double %x)
  %r = call reassoc double @llvm.log2.f64(double %p)
  ret double %r
}
