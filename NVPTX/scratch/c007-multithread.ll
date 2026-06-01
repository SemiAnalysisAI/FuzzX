target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }

declare i32 @llvm.nvvm.read.ptx.sreg.tid.x()

define ptx_kernel void @kern_mt(ptr byval(%struct.S) align 8 %p, i1 %c, ptr %out) {
entry:
  %sel = select i1 %c, ptr %p, ptr %p
  %old = atomicrmw add ptr %sel, i32 7 seq_cst
  %tid = call i32 @llvm.nvvm.read.ptx.sreg.tid.x()
  %idx = zext i32 %tid to i64
  %gep = getelementptr i32, ptr %out, i64 %idx
  store i32 %old, ptr %gep, align 4
  ret void
}
