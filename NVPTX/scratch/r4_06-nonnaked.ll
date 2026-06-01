target triple = "nvptx64-nvidia-cuda"

define void @f() {
entry:
  call void @sideeffect()
  unreachable
}

declare void @sideeffect()
