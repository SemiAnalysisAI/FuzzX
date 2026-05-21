target triple = "x86_64-unknown-linux-gnu"
declare {double, i64} @llvm.frexp.f64.i64(double)
define i64 @frexp_i64(double %x) {
  %r = call {double, i64} @llvm.frexp.f64.i64(double %x)
  %e = extractvalue {double, i64} %r, 1
  ret i64 %e
}
