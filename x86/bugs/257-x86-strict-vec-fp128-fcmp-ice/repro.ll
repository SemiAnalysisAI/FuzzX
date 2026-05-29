define <2 x i1> @t(<2 x fp128> %a, <2 x fp128> %b) #0 {
  %r = call <2 x i1> @llvm.experimental.constrained.fcmp.v2f128(<2 x fp128> %a, <2 x fp128> %b, metadata !"olt", metadata !"fpexcept.strict")
  ret <2 x i1> %r
}
declare <2 x i1> @llvm.experimental.constrained.fcmp.v2f128(<2 x fp128>, <2 x fp128>, metadata, metadata)
attributes #0 = { strictfp }
