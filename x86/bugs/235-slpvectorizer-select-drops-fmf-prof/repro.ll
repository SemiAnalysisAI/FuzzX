target triple = "x86_64-unknown-linux-gnu"
define void @f(ptr %d, <4 x i1> %m, <4 x float> %x, <4 x float> %y) {
  %e1 = extractelement <4 x i1> %m, i32 0
  %x1 = extractelement <4 x float> %x, i32 0
  %y1 = extractelement <4 x float> %y, i32 0
  %s1 = select nnan i1 %e1, float %x1, float %y1
  %p1 = getelementptr float, ptr %d, i32 0
  store float %s1, ptr %p1, align 4
  %e2 = extractelement <4 x i1> %m, i32 1
  %x2 = extractelement <4 x float> %x, i32 1
  %y2 = extractelement <4 x float> %y, i32 1
  %s2 = select nnan i1 %e2, float %x2, float %y2
  %p2 = getelementptr float, ptr %d, i32 1
  store float %s2, ptr %p2, align 4
  %e3 = extractelement <4 x i1> %m, i32 2
  %x3 = extractelement <4 x float> %x, i32 2
  %y3 = extractelement <4 x float> %y, i32 2
  %s3 = select nnan i1 %e3, float %x3, float %y3
  %p3 = getelementptr float, ptr %d, i32 2
  store float %s3, ptr %p3, align 4
  %e4 = extractelement <4 x i1> %m, i32 3
  %x4 = extractelement <4 x float> %x, i32 3
  %y4 = extractelement <4 x float> %y, i32 3
  %s4 = select nnan i1 %e4, float %x4, float %y4
  %p4 = getelementptr float, ptr %d, i32 3
  store float %s4, ptr %p4, align 4
  ret void
}
