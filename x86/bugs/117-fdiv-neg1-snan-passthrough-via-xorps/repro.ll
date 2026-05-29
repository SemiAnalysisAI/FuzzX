; visitFDIV `fdiv X, -1.0` -> visitFMUL `fmul X, -1.0` -> visitFSUB `fsub -0.0, X`
; -> visitFSUB returns `fneg X`. All without `nnan`. The in-source FIXME at
; DAGCombiner.cpp:19057 documents the FSUB step.
define double @fdiv_neg1(double %x) {
  %r = fdiv double %x, -1.0
  ret double %r
}
