target triple = "nvptx64-nvidia-cuda"

define void @f() #0 {
entry:
  call void @sideeffect()
  unreachable
}

declare void @sideeffect()

attributes #0 = { naked }
