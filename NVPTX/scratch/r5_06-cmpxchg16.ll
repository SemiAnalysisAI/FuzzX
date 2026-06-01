define i16 @reg_cmpxchg_i16_block(ptr %p, i16 %c, i16 %s) {
  %pair = cmpxchg ptr %p, i16 %c, i16 %s syncscope("block") seq_cst seq_cst
  %r = extractvalue { i16, i1 } %pair, 0
  ret i16 %r
}
