target triple = "x86_64-unknown-linux-gnu"
define float @f(float %a, float %b) {
entry:
  br label %h
h:
  %i = phi i32 [0, %entry], [%inext, %h]
  %y = fmul nnan ninf nsz reassoc float %a, %b
  %inext = add i32 %i, 1
  %c = icmp slt i32 %inext, 10
  br i1 %c, label %h, label %exit
exit:
  ret float %y
}
