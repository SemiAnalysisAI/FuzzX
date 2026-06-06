declare <2 x i32> @g()
declare i32 @__gxx_personality_v0(...)

define i32 @scalarizephi_invoke_incoming_crash(i1 %c) personality ptr @__gxx_personality_v0 {
entry:
  %inv = invoke <2 x i32> @g()
          to label %loop unwind label %lpad

loop:
  %phi = phi <2 x i32> [ %inv, %entry ], [ %add, %loop ]
  %e = extractelement <2 x i32> %phi, i64 0
  %add = add <2 x i32> %phi, <i32 1, i32 1>
  br i1 %c, label %loop, label %exit

exit:
  ret i32 %e

lpad:
  %lp = landingpad { ptr, i32 }
          cleanup
  unreachable
}
