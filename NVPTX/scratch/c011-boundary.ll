declare i32 @llvm.nvvm.read.ptx.sreg.cluster.nctarank()

define ptx_kernel i32 @k() "nvvm.maxclusterrank"="4294967294" {
  %1 = call i32 @llvm.nvvm.read.ptx.sreg.cluster.nctarank()
  ret i32 %1
}
