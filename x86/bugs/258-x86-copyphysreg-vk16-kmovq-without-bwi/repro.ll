target triple = "x86_64-unknown-linux-gnu"
define void @ksp() {
  %x = call i16 asm sideeffect "kxorw %k1, %k1, $0", "={k1}"()
  call void asm sideeffect "kmovw $0, %k3", "{k2}"(i16 %x)
  ret void
}
