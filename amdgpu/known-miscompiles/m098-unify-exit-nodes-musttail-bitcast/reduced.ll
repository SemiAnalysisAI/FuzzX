target triple = "amdgcn-amd-amdhsa"

declare ptr @callee_ptr(i32)

; AMDGPUUnifyDivergentExitNodes musttail detection: looks at
; ReturnInst::getPrevNode(). Verifier permits an OPTIONAL bitcast between
; musttail call and the ret. getPrevNode() then returns the bitcast, not
; the CallInst, so dyn_cast_or_null<CallInst> returns nullptr and the block
; is NOT skipped.
define ptr @fuzz(i32 %tid) {
entry:
  %div = icmp slt i32 %tid, 0
  br i1 %div, label %tail, label %normal

normal:
  ret ptr null

tail:
  %r1 = musttail call ptr @callee_ptr(i32 %tid)
  %r1c = bitcast ptr %r1 to ptr
  ret ptr %r1c
}
