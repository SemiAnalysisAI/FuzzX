declare void @llvm.memcpy.element.unordered.atomic.p0.p0.i64(ptr, ptr, i64, i32 immarg)
define void @f(ptr %d, ptr %s) {
  call void @llvm.memcpy.element.unordered.atomic.p0.p0.i64(ptr align 4 %d, ptr align 4 %s, i64 4, i32 1)
  ret void
}
