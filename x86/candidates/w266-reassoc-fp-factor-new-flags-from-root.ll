; FP version of the factor pattern.

define double @test(double %a, double %b, double %c, double %d, double %e) {
  ; All intermediate ops have ONLY reassoc nsz.
  %t1 = fadd reassoc nsz double %a, %b
  %t2 = fadd reassoc nsz double %t1, %a
  %t3 = fadd reassoc nsz double %t2, %c
  %t4 = fadd reassoc nsz double %t3, %d
  ; Root has more: also nnan, ninf, arcp.
  %r  = fadd reassoc nsz nnan ninf arcp double %t4, %e
  ret double %r
}
