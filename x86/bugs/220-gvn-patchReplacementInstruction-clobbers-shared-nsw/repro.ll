target triple = "x86_64-unknown-linux-gnu"
declare void @use(i32)
declare {i32, i1} @llvm.sadd.with.overflow.i32(i32, i32)
define void @f(i32 %x, i32 %y) {
  %addnsw = add nsw i32 %x, %y
  call void @use(i32 %addnsw)            ; pre-existing user
  %ov = call {i32, i1} @llvm.sadd.with.overflow.i32(i32 %x, i32 %y)
  %res = extractvalue {i32, i1} %ov, 0
  call void @use(i32 %res)
  ret void
}
