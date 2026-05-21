target triple = "x86_64-unknown-linux-gnu"
declare double @llvm.experimental.constrained.fadd.f64(double, double, metadata, metadata)
define double @f() strictfp {
  %r = tail call nnan double @llvm.experimental.constrained.fadd.f64(double 0x7FF8000000000001, double 1.0, metadata !"round.dynamic", metadata !"fpexcept.strict") strictfp
  ret double %r
}
