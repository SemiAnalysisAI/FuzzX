target triple = "x86_64-unknown-linux-gnu"
declare double @fdim(double, double) memory(none)
define double @test_inf_inf() {
  %r = call double @fdim(double 0x7FF0000000000000, double 0x7FF0000000000000)
  ret double %r
}
define double @test_ninf_ninf() {
  %r = call double @fdim(double 0xFFF0000000000000, double 0xFFF0000000000000)
  ret double %r
}
