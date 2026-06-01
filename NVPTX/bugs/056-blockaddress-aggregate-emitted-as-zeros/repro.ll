define void @bar() {
entry:
  br label %lbl
lbl:
  ret void
}
@ba_arr = global [1 x ptr] [ptr blockaddress(@bar, %lbl)]
