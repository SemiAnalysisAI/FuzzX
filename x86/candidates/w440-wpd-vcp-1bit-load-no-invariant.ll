target datalayout = "e-p:64:64"

@vt1 = constant [3 x ptr] [ ptr @vf0i1, ptr @vfA, ptr @vfB ], !type !0
@vt2 = constant [3 x ptr] [ ptr @vf1i1, ptr @vfA, ptr @vfB ], !type !0
@vt3 = constant [3 x ptr] [ ptr @vf0i1, ptr @vfA, ptr @vfB ], !type !0
@vt4 = constant [3 x ptr] [ ptr @vf1i1, ptr @vfA, ptr @vfB ], !type !0
@vt5 = constant [3 x ptr] [ ptr @vf0i1, ptr @vfA, ptr @vfB ], !type !0

define i1 @vf0i1(ptr %this) readnone { ret i1 0 }
define i1 @vf1i1(ptr %this) readnone { ret i1 1 }
define void @vfA(ptr %this) { ret void }
define void @vfB(ptr %this) { ret void }

define i1 @call(ptr %obj) {
  %vtable = load ptr, ptr %obj, !invariant.load !100
  %p = call i1 @llvm.type.test(ptr %vtable, metadata !"typeid")
  call void @llvm.assume(i1 %p)
  %fptr = load ptr, ptr %vtable, !invariant.load !100
  %result = call i1 %fptr(ptr %obj)
  ret i1 %result
}

declare i1 @llvm.type.test(ptr, metadata)
declare void @llvm.assume(i1)

!0 = !{i32 0, !"typeid"}
!100 = !{}
