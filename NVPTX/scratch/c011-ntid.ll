declare i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
define ptx_kernel i32 @k() "nvvm.maxntid"="4294967295" {
  %1 = call i32 @llvm.nvvm.read.ptx.sreg.ntid.x()
  ret i32 %1
}
