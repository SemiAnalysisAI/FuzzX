; ArgumentPromotion strips sret -> noalias even when no arg is actually promoted.
; All pointer args here are unpromotable (escaped to @opaque), so
; promoteArguments() ultimately returns nullptr at the "ArgsToPromote.empty()"
; check -- but the sret attribute has already been rewritten in-place to
; noalias on both the function and every direct call site.
;
; Run:
;   opt -passes=argpromotion -S w415-ap-sret-strip.ll
;
; Expected (current LLVM 23.0.0git):
;   - %callee param loses sret(%S), gains noalias
;   - The direct call in @caller loses sret(%S), gains noalias
;   - @caller's own sret attribute is correctly left alone (different argument)

target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%S = type { i32, i32 }

declare void @opaque(ptr)

define internal void @callee(ptr sret(%S) %ret) {
  call void @opaque(ptr %ret)           ; opaque escape -> findArgParts() rejects
  ret void
}

define void @caller(ptr sret(%S) %ret) {
  call void @callee(ptr sret(%S) %ret)
  ret void
}
