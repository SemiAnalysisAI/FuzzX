define i32 @t(bfloat %a, bfloat %b) #0 {
  %r = call i1 @llvm.experimental.constrained.fcmp.bf16(bfloat %a, bfloat %b, metadata !"olt", metadata !"fpexcept.strict")
  %ext = zext i1 %r to i32
  ret i32 %ext
}
declare i1 @llvm.experimental.constrained.fcmp.bf16(bfloat, bfloat, metadata, metadata)
attributes #0 = { strictfp }
