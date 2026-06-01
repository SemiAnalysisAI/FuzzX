target triple = "nvptx64-nvidia-cuda"

; Read every element to see the full access pattern vs declared slot size.
define ptx_kernel void @kern_all(<8 x i1> %a, ptr %out) {
  %e0 = extractelement <8 x i1> %a, i32 0
  %e1 = extractelement <8 x i1> %a, i32 1
  %e7 = extractelement <8 x i1> %a, i32 7
  %z0 = zext i1 %e0 to i32
  %z1 = zext i1 %e1 to i32
  %z7 = zext i1 %e7 to i32
  %s1 = add i32 %z0, %z1
  %s2 = add i32 %s1, %z7
  store i32 %s2, ptr %out
  ret void
}
