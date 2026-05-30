target triple = "x86_64-unknown-linux-gnu"

; sitofp/uitofp of a wide integer whose magnitude exceeds the largest finite
; float must produce +/-Inf. ExpandIRInsts computes the result exponent as
; `(unbiasedExp << MantissaWidth) + bias` with no overflow check, so once the
; exponent overflows the FP exponent field it wraps into garbage instead of Inf.

define float @ui129tofloat(i129 %a) {
  %conv = uitofp i129 %a to float
  ret float %conv
}

define float @si129tofloat(i129 %a) {
  %conv = sitofp i129 %a to float
  ret float %conv
}

; Self-contained miscompile: 2^200 >> FLT_MAX (~3.4e38). uitofp must give +Inf
; (0x7F800000) but the unfixed expansion returns 0xA4000000 (~ -2.66e-17).
define i32 @const_overflow() {
  %r = uitofp i256 1606938044258990275541962092341162602522202993782792835301376 to float
  %b = bitcast float %r to i32
  ret i32 %b
}
