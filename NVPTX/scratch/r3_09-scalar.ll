target triple = "nvptx64-nvidia-cuda"

; Scalar i1 kernel param - the well-understood baseline
define ptx_kernel void @kscalar(i1 %a, ptr %out) {
  %z = zext i1 %a to i32
  store i32 %z, ptr %out
  ret void
}

; i8 vector for contrast
define ptx_kernel void @kv8i8(<8 x i8> %a, ptr %out) {
  %e7 = extractelement <8 x i8> %a, i32 7
  %z = zext i8 %e7 to i32
  store i32 %z, ptr %out
  ret void
}
