declare double @llvm.tan.f64(double)
declare double @llvm.cos.f64(double)

define double @tan_times_cos(double %x) {
  %t = call double @llvm.tan.f64(double %x)
  %c = call double @llvm.cos.f64(double %x)
  %r = fmul contract double %t, %c
  ret double %r
}
