; w665 repro: `or disjoint (add nsw A, C), (B & ~C)` is rewritten by
; foldAddLikeCommutative to `add nsw A, (or B, C)`, but the new add
; can signed-overflow even when the source is non-poison.
;
; Concrete inputs that miscompile: a=100, b_in=130
;   source: lhs = add nsw 100, 5 = 105 (no overflow, ≤127)
;           rhs = and 130, 250 = 130
;           or disjoint 105, 130 = 235 = -21 (i8, disjoint OK)
;   target: or 130, 5 = 135
;           add nsw 100, 135 = 235 math (>127) -> SIGNED OVERFLOW -> POISON
;
; Source = -21, Target = poison. Optimization introduced poison.

define i8 @bug_or_disjoint(i8 %a, i8 %b_in) {
  %lhs = add nsw i8 %a, 5
  %rhs = and i8 %b_in, 250         ; ~5 in i8
  %r   = or disjoint i8 %lhs, %rhs
  ret i8 %r
}

; Run: opt -passes=instcombine -S bug.ll
; Output shows: %r = add nsw i8 %a, (or %b_in, 5)  -- NSW is wrong.
