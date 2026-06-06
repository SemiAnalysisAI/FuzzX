define void @autogen_SD232(ptr %p) {
entry:
  %v = load <1 x i64>, ptr %p, align 8
  store i8 poison, ptr %p, align 1
  br label %join

join:
  store <1 x i64> %v, ptr %p, align 8
  ret void
}
