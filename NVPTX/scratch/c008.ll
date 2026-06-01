target triple = "nvptx64-nvidia-cuda"

define i32 @f2u_i1_zext(float %a) {
  %r = fptoui float %a to i1
  %z = zext i1 %r to i32
  ret i32 %z
}

define i32 @f2s_i1_zext(float %a) {
  %r = fptosi float %a to i1
  %z = zext i1 %r to i32
  ret i32 %z
}

define i32 @f2u_i1_zext_half(half %a) {
  %r = fptoui half %a to i1
  %z = zext i1 %r to i32
  ret i32 %z
}

define i32 @f2u_i1_zext_double(double %a) {
  %r = fptoui double %a to i1
  %z = zext i1 %r to i32
  ret i32 %z
}
