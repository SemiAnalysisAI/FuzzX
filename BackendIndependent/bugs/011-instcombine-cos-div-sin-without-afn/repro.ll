declare double @llvm.sin.f64(double)
declare double @llvm.cos.f64(double)

define double @cos_div_sin(double %x) {
  %c = call double @llvm.cos.f64(double %x)
  %s = call double @llvm.sin.f64(double %x)
  %r = fdiv reassoc double %c, %s
  ret double %r
}
