declare i2 @llvm.ctpop.i2(i2)
declare i2 @llvm.ctlz.i2(i2, i1 immarg)

define i2 @log2ceil_i2(i2 %a) {
  %pop = call i2 @llvm.ctpop.i2(i2 %a)
  %cmp = icmp ne i2 %pop, 1
  %z = zext i1 %cmp to i2
  %lz = call i2 @llvm.ctlz.i2(i2 %a, i1 true)
  %xor = xor i2 %lz, 1
  %add = add i2 %z, %xor
  ret i2 %add
}
