target triple = "x86_64-unknown-linux-gnu"
declare double @fmod(double, double)
declare void @llvm.assume(i1)
define double @nan_fmod(double %x) {
  %k = fcmp uno double %x, 0.0
  call void @llvm.assume(i1 %k)         ; x is NaN
  %ninf = fcmp one double %x, 0x7FF0000000000000
  call void @llvm.assume(i1 %ninf)
  %ninf2 = fcmp one double %x, 0xFFF0000000000000
  call void @llvm.assume(i1 %ninf2)
  %r = call double @fmod(double %x, double 1.0)
  ret double %r
}
