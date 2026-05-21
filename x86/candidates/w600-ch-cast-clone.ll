target triple = "x86_64-unknown-linux-gnu"

; Each `trunc nuw` user wraps a big i64 constant.  Each trunc is used by
; a non-cast user (the add i32 below).  ConstantHoisting collects the
; underlying i64 constant from inside the cast and rebases it.
;
; At emit time it clones the `trunc nuw` and substitutes its operand
; with Mat = base + offset, but Mat = base+offset for an i64 may set the
; high 32 bits and violate the nuw flag (which claims that the original
; value's upper bits truncated to zero), producing poison for downstream
; consumers.

define i32 @ch_trunc_nuw_clone(i32 %y) {
entry:
  ; Six i64 constants whose high bit is set differently so Mat can break nuw.
  %t0 = trunc nuw i64 4503599627370496 to i32     ; 1 << 52
  %t1 = trunc nuw i64 4503599627370497 to i32
  %t2 = trunc nuw i64 4503599627370498 to i32
  %t3 = trunc nuw i64 4503599627370499 to i32
  %t4 = trunc nuw i64 4503599627370500 to i32
  %t5 = trunc nuw i64 4503599627370501 to i32
  %a0 = add i32 %t0, %y
  %a1 = add i32 %t1, %y
  %a2 = add i32 %t2, %y
  %a3 = add i32 %t3, %y
  %a4 = add i32 %t4, %y
  %a5 = add i32 %t5, %y
  %s01 = add i32 %a0, %a1
  %s23 = add i32 %a2, %a3
  %s45 = add i32 %a4, %a5
  %s0123 = add i32 %s01, %s23
  %r = add i32 %s0123, %s45
  ret i32 %r
}
