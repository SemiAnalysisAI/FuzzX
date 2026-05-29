define i32 @f(ptr %p, i32 %x) {
  %v = load i32, ptr %p
  %c = icmp slt i32 %v, 0
  br i1 %c, label %T, label %F
T: ret i32 1
F: ret i32 0
}
