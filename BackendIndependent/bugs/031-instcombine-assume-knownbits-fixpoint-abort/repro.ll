define i8 @assume_knownbits_fixpoint(i32 %t0, i16 %insert, i64 %e, i8 %i162) {
  %b = icmp eq i32 %t0, 0
  %b2 = icmp eq i16 %insert, 0
  %t1 = or i1 %b, %b2
  %ext = zext i1 %t1 to i32
  %and = and i32 %t0, %ext
  %conv13 = zext i32 %and to i64
  %xor = xor i64 %conv13, 140
  %conv16 = sext i8 %i162 to i64
  %sub17 = sub i64 %conv16, %e
  %sext = shl i64 %sub17, 32
  %conv18 = ashr exact i64 %sext, 32
  %cmp = icmp sge i64 %xor, %conv18
  %conv19 = zext i1 %cmp to i16
  %or21 = or i16 %insert, %conv19
  %trunc44 = trunc i16 %or21 to i8
  %inc = add i8 %i162, %trunc44
  %tobool23.not = icmp eq i16 %or21, 0
  call void @llvm.assume(i1 %tobool23.not)
  ret i8 %inc
}

declare void @llvm.assume(i1)
