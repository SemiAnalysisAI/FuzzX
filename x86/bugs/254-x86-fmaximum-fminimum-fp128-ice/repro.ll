define fp128 @t(fp128 %a, fp128 %b) {
  %r = call fp128 @llvm.maximum.f128(fp128 %a, fp128 %b)
  ret fp128 %r
}
declare fp128 @llvm.maximum.f128(fp128, fp128)
