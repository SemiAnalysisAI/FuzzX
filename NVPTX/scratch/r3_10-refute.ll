target triple = "nvptx64-nvidia-cuda"

declare i32 @llvm.nvvm.atomic.max.gen.i.cta.i32(ptr, i32)
declare i64 @llvm.nvvm.atomic.max.gen.i.cta.i64(ptr, i64)
declare i32 @llvm.nvvm.atomic.min.gen.i.cta.i32(ptr, i32)
declare i32 @llvm.nvvm.atomic.max.gen.i.sys.i32(ptr, i32)

define i32 @umax32(ptr %p, i32 %v) {
  %r = call i32 @llvm.nvvm.atomic.max.gen.i.cta.i32(ptr %p, i32 %v)
  ret i32 %r
}

define i64 @umax64(ptr %p, i64 %v) {
  %r = call i64 @llvm.nvvm.atomic.max.gen.i.cta.i64(ptr %p, i64 %v)
  ret i64 %r
}

define i32 @umin32(ptr %p, i32 %v) {
  %r = call i32 @llvm.nvvm.atomic.min.gen.i.cta.i32(ptr %p, i32 %v)
  ret i32 %r
}

define i32 @umax32_sys(ptr %p, i32 %v) {
  %r = call i32 @llvm.nvvm.atomic.max.gen.i.sys.i32(ptr %p, i32 %v)
  ret i32 %r
}
