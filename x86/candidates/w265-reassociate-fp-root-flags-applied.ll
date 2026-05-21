; Force reassociate to actually rewrite something.
; %t1 has minimum FMF (reassoc nsz only).
; Root has additional 'nnan ninf'.
; The expression tree has shape (a+b)+(c+d) - this should canonicalize.

define double @test(double %a, double %b, double %c, double %d) {
  %t1 = fadd reassoc nsz double %a, %b
  %t2 = fadd reassoc nsz double %c, %d
  %r  = fadd reassoc nsz nnan ninf double %t2, %t1
  ret double %r
}
