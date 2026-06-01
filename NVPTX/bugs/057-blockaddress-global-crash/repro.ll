define void @bar() {
entry:
  br label %lbl
lbl:
  ret void
}
@ba = global ptr blockaddress(@bar, %lbl)
