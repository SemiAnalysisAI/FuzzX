declare <2 x i8> @llvm.fshl.v2i8(<2 x i8>, <2 x i8>, <2 x i8>)

define i8 @fshl_zero_vector_lane0(<2 x i8> %x) {
  %r = call <2 x i8> @llvm.fshl.v2i8(
    <2 x i8> zeroinitializer,
    <2 x i8> %x,
    <2 x i8> <i8 0, i8 1>)
  %fr = freeze <2 x i8> %r
  %e = extractelement <2 x i8> %fr, i32 0
  ret i8 %e
}
