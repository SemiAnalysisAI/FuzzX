target triple = "nvptx64-nvidia-cuda"

define void @f(i1 %p0, i1 %p1) #0 {
entry:
  br i1 %p0, label %cont, label %maybe
maybe:
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

attributes #0 = { naked }
