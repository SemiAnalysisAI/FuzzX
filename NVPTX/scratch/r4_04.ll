define ptx_kernel i32 @t() "nvvm.maxntid"="0" {
  %ntid.x = call i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
  ret i32 %ntid.x
}
declare i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
