target triple = "x86_64-unknown-linux-gnu"

; Non-saturating fptoui half -> i256 (regular fptoui, not intrinsic).
; Half range fits in i32, so this is a fast-path -> fptoui to i32, then zext.
define i256 @ui(half %x) {
  %r = fptoui half %x to i256
  ret i256 %r
}
