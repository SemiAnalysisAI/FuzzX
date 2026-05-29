target triple = "x86_64-unknown-linux-gnu"
define i32 @t(i32 %x) {
  %a = ashr exact i32 %x, 3
  %m = and i32 %a, 255
  ret i32 %m
}
