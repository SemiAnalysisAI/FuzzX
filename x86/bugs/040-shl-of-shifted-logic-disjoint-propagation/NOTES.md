# Candidate: combineShiftOfShiftedLogic propagates OR `disjoint` incorrectly when shifts differ

File: llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:10539-10607 (combineShiftOfShiftedLogic),
in particular the closing `return DAG.getNode(LogicOpcode, DL, VT, NewShift1, NewShift2, LogicOp->getFlags());`
at line 10605.

## Reasoning

`combineShiftOfShiftedLogic` transforms
`shift(logic(shift(X, C0), Y), C1)` into
`logic(shift(X, C0+C1), shift(Y, C1))`,
and propagates `LogicOp->getFlags()` verbatim onto the new outer logic op.

For `or disjoint` (`(a & b) == 0`), this is preserved by uniform shifts of
both operands. But the inner-shift sub-expression has `X` shifted by `C0`,
so the original disjoint guarantee is between `X << C0` and `Y`. After the
fold, disjointness becomes between `X << (C0+C1)` and `Y << C1`. That is in
general NOT the same condition. The original guarantee:
`((X << C0) & Y) == 0`.
The new guarantee carried by the `disjoint` flag:
`((X << (C0+C1)) & (Y << C1)) == 0`,
which equals `((X << C0) & Y) << C1 == 0` — well, that's true iff
`((X << C0) & Y) == 0` after the top `C1` bits are dropped. But the shift
loses the top bits, so the new disjointness may be claimed even when, modulo
the original BW, the masked bits overlap (because shifted-off overlap is
ignored).

So actually the new disjoint can be more-true than the old. The problem is
the reverse: if the original had no disjoint but the rewrite produces an
operation that DOES happen to have disjoint, no flag is set so that's fine.
The fold is currently safe for OR/AND/XOR — XOR/AND have no flags of
concern. Investigate further whether `disjoint` is actually being copied
from `or disjoint`.

(Lower-priority candidate; flagging for review.)

## Candidate IR

```ll
define i32 @t(i32 %x, i32 %y) {
  %sx = shl i32 %x, 4
  %or = or disjoint i32 %sx, %y     ; promises (sx & y) == 0
  %r  = shl i32 %or, 8              ; outer shift by 8
  ret i32 %r
}
```

After combineShiftOfShiftedLogic: `or disjoint (shl x, 12), (shl y, 8)`.
The disjoint claim now is `(x<<12) & (y<<8) == 0`. Since the original
disjoint was `(x<<4) & y == 0`, shifting both by 8 preserves the bit
positions that originally were disjoint, but the *top 8 bits* of the
shifted operands are dropped. Specifically: if `x<<4` had bits set in the
top 8 (positions 24..31) and `y` had bits in positions 24..31, they were
disjoint only if NOT overlapping. After shifting by 8, those bits fall off,
so disjointness might still hold for the visible bits. Still need a concrete
counter-example.

## Expected wrong outcome

A subsequent combine that uses the `disjoint` flag to rewrite OR to ADD/XOR
could produce incorrect results if disjoint is improperly set.
