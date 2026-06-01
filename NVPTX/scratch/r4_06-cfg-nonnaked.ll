target triple = "nvptx64-nvidia-cuda"

declare i32 @llvm.nvvm.read.ptx.sreg.tid.x()
declare void @sideeffect()
declare void @barsync()

define void @f() {
entry:
  %t = call i32 @llvm.nvvm.read.ptx.sreg.tid.x()
  %p = icmp eq i32 %t, 0
  br i1 %p, label %unlikely, label %cont

unlikely:
  call void @sideeffect()
  unreachable

cont:
  call void @barsync()
  ret void
}

attributes = { naked }
