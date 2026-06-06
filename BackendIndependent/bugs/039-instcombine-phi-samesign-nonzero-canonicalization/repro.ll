declare void @use_i1(i1)

define i1 @h(i2 %sel, i32 %y) {
entry:
  switch i2 %sel, label %unk [
    i2 0, label %neg
    i2 1, label %pos
  ]

neg:
  br label %join

pos:
  br label %join

unk:
  br label %join

join:
  %v = phi i32 [ -1, %neg ], [ 1, %pos ], [ %y, %unk ]
  %cmp1 = icmp samesign ne i32 %v, 0
  %cmp2 = icmp eq i32 %v, 0
  call void @use_i1(i1 %cmp2)
  ret i1 %cmp1
}
