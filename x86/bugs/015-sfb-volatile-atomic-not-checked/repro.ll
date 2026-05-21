; Source-confirmed bug — see NOTES.md. The triggering MIR pattern (volatile
; 16-byte XMM load + small blocker store + volatile XMM store) is sensitive
; to upstream MIR shape and not always reproducible from IR alone.
define void @vol_copy(ptr noundef %dst, ptr noundef %src) {
  %v = load volatile <16 x i8>, ptr %src, align 16
  store i8 0, ptr %dst, align 1
  store volatile <16 x i8> %v, ptr %dst, align 16
  ret void
}
