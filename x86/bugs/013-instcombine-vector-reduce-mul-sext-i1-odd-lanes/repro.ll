declare i8 @llvm.vector.reduce.mul.v3i8(<3 x i8>)

define i8 @f(<3 x i1> %m) {
  %s = sext <3 x i1> %m to <3 x i8>
  %r = call i8 @llvm.vector.reduce.mul.v3i8(<3 x i8> %s)
  ret i8 %r
}
