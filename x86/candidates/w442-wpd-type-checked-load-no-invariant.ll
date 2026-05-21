target datalayout = "e-p:64:64"

@vt1 = constant [3 x ptr] [ ptr @vf1, ptr @vf2, ptr @vf3 ], !type !0

define void @vf1(ptr %this) { ret void }
define void @vf2(ptr %this) { ret void }
define void @vf3(ptr %this) { ret void }

; Use llvm.type.checked.load - this triggers scanTypeCheckedLoadUsers path.
define void @test(ptr %obj) {
  %vtable = load ptr, ptr %obj
  %1 = call {ptr, i1} @llvm.type.checked.load(ptr %vtable, i32 0, metadata !"typeid")
  %fp = extractvalue {ptr, i1} %1, 0
  %tt = extractvalue {ptr, i1} %1, 1
  call void @llvm.assume(i1 %tt)
  call void %fp(ptr %obj)
  ret void
}

declare {ptr, i1} @llvm.type.checked.load(ptr, i32, metadata)
declare void @llvm.assume(i1)

!0 = !{i32 0, !"typeid"}
