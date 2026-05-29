define bfloat @t(bfloat %a, bfloat %b) #0 {
  %r = call bfloat @llvm.experimental.constrained.fadd.bf16(bfloat %a, bfloat %b, metadata !"round.dynamic", metadata !"fpexcept.strict")
  ret bfloat %r
}
declare bfloat @llvm.experimental.constrained.fadd.bf16(bfloat, bfloat, metadata, metadata)
attributes #0 = { strictfp }
