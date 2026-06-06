define <8 x i1> @wide_load_compare_fixpoint(ptr %p) {
  store i8 0, ptr %p, align 1
  %l = load <8 x i8>, ptr %p, align 8
  store i8 0, ptr %p, align 1
  %c = icmp ule <8 x i8> zeroinitializer, %l
  ret <8 x i1> %c
}
