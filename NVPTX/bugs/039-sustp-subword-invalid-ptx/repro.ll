target triple = "nvptx64-nvidia-cuda"
declare void @llvm.nvvm.sust.p.1d.i8.trap(i64, i32, i16)
declare void @llvm.nvvm.sust.p.2d.i16.trap(i64, i32, i32, i16)
define ptx_kernel void @t_p_1d_i8(i64 %s, i32 %x, i16 %v) {
  tail call void @llvm.nvvm.sust.p.1d.i8.trap(i64 %s, i32 %x, i16 %v)
  ret void
}
define ptx_kernel void @t_p_2d_i16(i64 %s, i32 %x, i32 %y, i16 %v) {
  tail call void @llvm.nvvm.sust.p.2d.i16.trap(i64 %s, i32 %x, i32 %y, i16 %v)
  ret void
}
