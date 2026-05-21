; -mattr=+lvi-cfi should harden every ret-class instruction; the pass only
; matches X86::RET64 and silently leaves RETI64/LRET64/IRET64 un-hardened.

define void @f() {
  call void asm sideeffect "ret $$8", ""()   ; inline asm produces RETI64
  unreachable
}
