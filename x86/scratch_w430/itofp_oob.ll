target triple = "x86_64-unknown-linux-gnu"

define float @ui_huge(i256 %x) {
  %r = uitofp i256 %x to float
  ret float %r
}

define float @si_min(i256 %x) {
  %r = sitofp i256 %x to float
  ret float %r
}
