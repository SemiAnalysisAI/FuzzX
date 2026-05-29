target triple = "x86_64-unknown-linux-gnu"
define double @f(i1 %c, i32 %i, double %y) {
  %x = sitofp i32 %i to double
  %m = fmul ninf double %x, %y
  %r = select i1 %c, double %m, double %x
  ret double %r
}
