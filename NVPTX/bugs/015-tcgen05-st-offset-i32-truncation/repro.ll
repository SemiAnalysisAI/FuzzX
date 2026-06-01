target triple = "nvptx64-nvidia-cuda"

define void @st_big_offset(ptr addrspace(6) %taddr, <2 x i32> %stv2) {
  tail call void @llvm.nvvm.tcgen05.st.16x32bx2.x2(ptr addrspace(6) %taddr, i64 4294967298, <2 x i32> %stv2, i1 0)
  ret void
}
