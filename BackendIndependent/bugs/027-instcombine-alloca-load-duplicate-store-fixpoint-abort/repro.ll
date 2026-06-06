define i16 @alloca_load_exposes_duplicate_store(ptr %p) {
  %A = alloca <8 x i16>, align 16
  store float 0.000000e+00, ptr %p, align 4
  store float 0.000000e+00, ptr %A, align 4
  store float 0.000000e+00, ptr %p, align 4
  %L24 = load i16, ptr %A, align 2
  ret i16 %L24
}
