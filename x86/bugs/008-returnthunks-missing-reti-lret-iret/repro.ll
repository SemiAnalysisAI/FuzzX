target triple = "i686-unknown-linux-gnu"

; -mfunction-return=thunk-extern (encoded as fn_ret_thunk_extern) is the
; kernel's Retbleed mitigation. The X86ReturnThunks pass should rewrite EVERY
; return instruction in the function to `jmp __x86_return_thunk`. It only
; matches RET32/RET64; stdcall (`retl $N` / RETI32 / RETI64) survives.

define x86_stdcallcc i32 @foo(i32 %x) #0 {
  ret i32 %x
}

attributes #0 = { fn_ret_thunk_extern }
