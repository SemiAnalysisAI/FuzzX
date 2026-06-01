target triple = "nvptx64-nvidia-cuda"

declare i32 @llvm.nvvm.ldu.global.i.i32.p0(ptr, i32)

define i32 @t(ptr %p, i32 %align) {
  %v = call i32 @llvm.nvvm.ldu.global.i.i32.p0(ptr %p, i32 %align)
  ret i32 %v
}
