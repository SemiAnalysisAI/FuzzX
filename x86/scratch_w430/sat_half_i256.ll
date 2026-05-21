; ModuleID = 'sat_half_i256.ll'
target triple = "x86_64-unknown-linux-gnu"

define i256 @ui_sat(half %x) {
  %r = call i256 @llvm.fptoui.sat.i256.f16(half %x)
  ret i256 %r
}

define i256 @si_sat(half %x) {
  %r = call i256 @llvm.fptosi.sat.i256.f16(half %x)
  ret i256 %r
}

declare i256 @llvm.fptoui.sat.i256.f16(half)
declare i256 @llvm.fptosi.sat.i256.f16(half)
