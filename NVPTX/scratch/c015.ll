declare <2 x i32> @llvm.nvvm.tcgen05.ld.16x32bx2.x2(ptr addrspace(6), i64 immarg, i1 immarg)

define <2 x i32> @t(ptr addrspace(6) %taddr) {
  ; offset = 0x1_0000_0002 = 4294967298; low 32 bits are 2, bit 32 set
  %v = tail call <2 x i32> @llvm.nvvm.tcgen05.ld.16x32bx2.x2(ptr addrspace(6) %taddr, i64 4294967298, i1 0)
  ret <2 x i32> %v
}
