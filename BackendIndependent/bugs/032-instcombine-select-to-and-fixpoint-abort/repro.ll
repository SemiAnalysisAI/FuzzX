define void @select_to_and_fixpoint(ptr %A) {
  %L = load i177, ptr %A
  %B5 = udiv i177 %L, -1
  %B4 = add i177 %B5, -1
  %B2 = add i177 %B4, -1
  %G11 = getelementptr i177, ptr %A, i177 %B2
  %L7 = load i177, ptr %G11
  %B6 = mul i177 %B5, %B2
  %B24 = ashr i177 %L7, %B6
  %B36 = and i177 %L7, %B4
  %C17 = icmp sgt i177 %B36, %B24
  %G62 = getelementptr i177, ptr %G11, i1 %C17
  %B28 = urem i177 %B24, %B6
  store i177 %B28, ptr %G62
  ret void
}
