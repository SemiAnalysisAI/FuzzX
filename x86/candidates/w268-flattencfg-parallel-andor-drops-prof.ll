@g = external global i32

; Force parallel-and: if (a && b) X
; Shape: entry -> bb_a (cond a) -> bb_b (cond b) -> if_then
;                 \-> exit              \-> exit

define void @test_parand(i1 %a, i1 %b, i32 %z) {
entry:
  br i1 %a, label %bb_b, label %exit, !prof !0

bb_b:
  br i1 %b, label %if_then, label %exit, !prof !1

if_then:
  store i32 %z, ptr @g, align 4
  br label %exit

exit:
  ret void
}

!0 = !{!"branch_weights", i32 1, i32 99}
!1 = !{!"branch_weights", i32 30, i32 70}
