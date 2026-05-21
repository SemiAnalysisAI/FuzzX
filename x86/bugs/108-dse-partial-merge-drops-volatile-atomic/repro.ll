define void @test_volatile(ptr %p) {
  store i32 0, ptr %p, align 4
  %p2 = getelementptr i8, ptr %p, i64 2
  store volatile i16 -1, ptr %p2, align 2
  ret void
}
define void @test_atomic(ptr %p) {
  store i32 0, ptr %p, align 4
  %p2 = getelementptr i8, ptr %p, i64 2
  store atomic i16 -1, ptr %p2 monotonic, align 2
  ret void
}
