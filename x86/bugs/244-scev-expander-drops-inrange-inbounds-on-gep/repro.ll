target triple = "x86_64-unknown-linux-gnu"
@vt = external constant [4 x ptr]
define ptr @f(i32 %n) {
entry:
  br label %loop
loop:
  %i = phi i32 [0, %entry], [%inext, %loop]
  %addr = getelementptr inbounds inrange(-8, 24) ptr, ptr getelementptr inbounds (i8, ptr @vt, i64 8), i32 %i
  %inext = add nsw i32 %i, 1
  %c = icmp slt i32 %inext, %n
  br i1 %c, label %loop, label %exit
exit:
  ret ptr %addr
}
