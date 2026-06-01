target triple = "nvptx64-nvidia-cuda"
declare i32 @llvm.nvvm.ldu.global.i.i32.p0(ptr, i32)
define i32 @t(ptr %p) {
  %v = call i32 @llvm.nvvm.ldu.global.i.i32.p0(ptr %p, i32 4)
  ret i32 %v
}
