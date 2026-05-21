; Two-word integer add. Under -global-isel, the X86InstructionSelector
; lowers G_UADDE's carry-in by emitting `CMP r, 1` against the SETcc byte.
; That sets CF = (r < 1 unsigned) = (r == 0), which is the *inverse* of
; the intended carry, so the subsequent ADC adds the wrong CF.
;
; This shows up at i128 add on x86_64 and at i64 add on i386.

define i128 @add128(i128 %a, i128 %b) {
  %r = add i128 %a, %b
  ret i128 %r
}

define i64 @add64_x32(i64 %a, i64 %b) {
  %r = add i64 %a, %b
  ret i64 %r
}
