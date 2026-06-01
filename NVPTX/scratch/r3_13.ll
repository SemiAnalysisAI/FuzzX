target triple = "nvptx64-nvidia-cuda"

; FTZ for f32 requested via function attribute (clang -fcuda-flush-denormals-to-zero
; sets the f32 denormal mode to preserve-sign). fpext half->float is an EXACT
; widening conversion: every f16 value (incl. subnormals) is a normal f32.
define float @ext_f16(half %a) #0 {
  %r = fpext half %a to float
  ret float %r
}

define float @ext_bf16(bfloat %a) #0 {
  %r = fpext bfloat %a to float
  ret float %r
}

attributes #0 = { denormal_fpenv(float: preservesign) }
