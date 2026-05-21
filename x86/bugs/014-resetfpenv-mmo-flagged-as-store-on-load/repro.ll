declare void @llvm.reset.fpenv()
define void @f(ptr %p) {
  %v0 = load i32, ptr %p
  call void @llvm.reset.fpenv()
  %v1 = load i32, ptr %p
  %s  = add i32 %v0, %v1
  store i32 %s, ptr %p
  ret void
}
