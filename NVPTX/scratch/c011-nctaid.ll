declare i32 @llvm.nvvm.read.ptx.sreg.cluster.nctaid.x()
define ptx_kernel i32 @k() "nvvm.maxclusterrank"="4294967295" {
  %1 = call i32 @llvm.nvvm.read.ptx.sreg.cluster.nctaid.x()
  ret i32 %1
}
