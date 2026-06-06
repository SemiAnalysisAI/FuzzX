define i32 @store_undef_overwrites_poison(ptr %p) {
  store i32 poison, ptr %p, align 4
  store i32 undef, ptr %p, align 4
  %v = load i32, ptr %p, align 4
  ret i32 %v
}
