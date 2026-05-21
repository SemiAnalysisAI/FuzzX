target triple = "x86_64-unknown-linux-gnu"
; sitofp -2^255 (INT_MIN of i256) to half should be -inf
define half @si_min() {
  %r = sitofp i256 -57896044618658097711785492504343953926634992332820282019728792003956564819968 to half
  ret half %r
}

; sitofp 2^255-1 (INT_MAX of i256) to half should be +inf
define half @si_max() {
  %r = sitofp i256 57896044618658097711785492504343953926634992332820282019728792003956564819967 to half
  ret half %r
}
