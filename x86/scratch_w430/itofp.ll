target triple = "x86_64-unknown-linux-gnu"

define bfloat @ui_bf(i256 %x) {
  %r = uitofp i256 %x to bfloat
  ret bfloat %r
}

define half @si_h(i256 %x) {
  %r = sitofp i256 %x to half
  ret half %r
}
