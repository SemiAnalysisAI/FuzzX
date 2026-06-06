define void @load_cse_scanlimit_fixpoint(ptr %p, ptr %A8) {
  %L7 = load i16, ptr %A8
  %G21 = getelementptr i16, ptr %A8, i8 -1
  %B11 = udiv i16 %L7, -1
  %G4 = getelementptr i16, ptr %A8, i16 %B11
  %L2 = load i16, ptr %G4
  %L = load i16, ptr %G4
  %B23 = mul i16 %B11, %B11
  %L4 = load i16, ptr %A8
  %B21 = sdiv i16 %L7, %L4
  %B7 = sub i16 0, %B21
  %B18 = mul i16 %B23, %B7
  %C10 = icmp ugt i16 %L, %B11
  %B20 = and i16 %L7, %L2
  %B1 = mul i1 %C10, true
  %C5 = icmp sle i16 %B21, %L
  %C11 = icmp ule i16 %B21, %L
  %C7 = icmp slt i16 %B20, 0
  %B29 = srem i16 %L4, %B18
  %B15 = add i1 %C7, %C10
  %B19 = add i1 %C11, %B15
  %C6 = icmp sge i1 %C11, %B19
  %B33 = or i16 %B29, %L4
  %C13 = icmp uge i1 %C5, %B1
  %C3 = icmp ult i1 %C13, %C6
  store i16 undef, ptr %G21
  %C18 = icmp ule i1 %C10, %C7
  %G26 = getelementptr i1, ptr null, i1 %C3
  store i16 %B33, ptr %p
  store i1 %C18, ptr %p
  store ptr %G26, ptr %p
  ret void
}
