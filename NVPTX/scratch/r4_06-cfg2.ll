target triple = "nvptx64-nvidia-cuda"

define void @f() #0 {
entry:
  %tid = call i32 @llvm.nvvm.read.ptx.sreg.tid.x()
  %p0 = icmp eq i32 %tid, 0
  br i1 %p0, label %cont, label %maybe
maybe:
  %p1 = icmp eq i32 %tid, 1
  br i1 %p1, label %unlikely, label %cont
unlikely:
  call void @sideeffect()
  unreachable
cont:
  call void @llvm.nvvm.barrier0()
  ret void
}

declare void @sideeffect()
declare void @llvm.nvvm.barrier0()
declare i32 @llvm.nvvm.read.ptx.sreg.tid.x()

attributes #0 = { naked }
