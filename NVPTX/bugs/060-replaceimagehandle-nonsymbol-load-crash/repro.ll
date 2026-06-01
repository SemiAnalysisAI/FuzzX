target triple = "nvptx64-unknown-nvcl"

declare i32 @llvm.nvvm.suld.1d.i32.trap(i64, i32)

define ptx_kernel void @foo(ptr %handleptr, ptr %red, i32 %idx) {
  %img = load i64, ptr %handleptr
  %val = tail call i32 @llvm.nvvm.suld.1d.i32.trap(i64 %img, i32 %idx)
  store i32 %val, ptr %red
  ret void
}

!nvvm.annotations = !{!1}
!1 = !{ptr @foo, !"rdwrimage", i32 0}
