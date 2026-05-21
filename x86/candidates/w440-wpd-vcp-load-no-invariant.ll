target datalayout = "e-p:64:64"

@vt1 = constant [3 x ptr] [
  ptr @vf0i16,
  ptr @vfA,
  ptr @vfB
], !type !0

@vt2 = constant [3 x ptr] [
  ptr @vf2i16,
  ptr @vfA,
  ptr @vfB
], !type !0

@vt3 = constant [3 x ptr] [
  ptr @vf5i16,
  ptr @vfA,
  ptr @vfB
], !type !0

define i16 @vf0i16(ptr %this) readnone { ret i16 0 }
define i16 @vf2i16(ptr %this) readnone { ret i16 2 }
define i16 @vf5i16(ptr %this) readnone { ret i16 5 }
define void @vfA(ptr %this) { ret void }
define void @vfB(ptr %this) { ret void }

; Call returns i16 - multiple vtables return different constants;
; WPD vcp should insert a load near the vtable to pick up the constant.
define i16 @call_vcp(ptr %obj) {
  %vtable = load ptr, ptr %obj, !invariant.load !100
  %p = call i1 @llvm.type.test(ptr %vtable, metadata !"typeid")
  call void @llvm.assume(i1 %p)
  %fptr = load ptr, ptr %vtable, !invariant.load !100
  %result = call i16 %fptr(ptr %obj)
  ret i16 %result
}

declare i1 @llvm.type.test(ptr, metadata)
declare void @llvm.assume(i1)

!0 = !{i32 0, !"typeid"}
!100 = !{}
