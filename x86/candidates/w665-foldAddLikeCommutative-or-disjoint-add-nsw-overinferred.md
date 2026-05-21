# w665: `foldAddLikeCommutative` infers `add nsw` for `or disjoint → add` when only `add nsw` LHS guarantee was on the *inner* sum

## Severity
Miscompile (introduces poison where original IR was a defined value).

## File / lines
`llvm/lib/Transforms/InstCombine/InstCombineAddSub.cpp`
- Buggy lines: **1355-1370** in `InstCombinerImpl::foldAddLikeCommutative`.
- Caller in focus file: `llvm/lib/Transforms/InstCombine/InstCombineAndOrXor.cpp`
  lines **4172-4180** in `InstCombinerImpl::visitOr`, where the `or
  disjoint` path calls `foldAddLikeCommutative(..., /*NSW=*/true,
  /*NUW=*/true)`.

```cpp
// (A + C) + (B & ~C) == A + (B | C)
if (match(LHS, m_c_Add(m_Value(A), m_APInt(C1))) &&
    match(RHS, m_c_And(m_Value(B), m_SpecificInt(~*C1)))) {
  if (!LHS->hasOneUse() && !RHS->hasOneUse())
    return nullptr;

  bool NSWOut = NSW && match(LHS, m_NSWAdd(m_Value(), m_Value()));   // <-- BUG
  bool NUWOut = NUW && match(LHS, m_NUWAdd(m_Value(), m_Value()));
  Value *NewOr =
      Builder.CreateOr(B, Constant::getIntegerValue(LHS->getType(), *C1));
  Instruction *NewAdd = BinaryOperator::CreateAdd(A, NewOr);
  NewAdd->setHasNoSignedWrap(NSWOut);
  NewAdd->setHasNoUnsignedWrap(NUWOut);
  return NewAdd;
}
```

When invoked from `visitOr` with `or disjoint X, Y` (which is the
*only* caller passing `NSW=true, NUW=true` from the AndOrXor path) the
fold rewrites
```
or disjoint (add nsw A, C1), (B & ~C1)
```
to
```
add nsw A, (or B, C1)
```

## Why this is wrong

The `add nsw A, C1` flag asserts that the **inner** sum `A + C1` does
not overflow signed. After the rewrite, the *new* outer add computes
`A + (B | C1)` which equals `(A + C1) + (B & ~C1)`. The disjoint
property guarantees this sum fits in the bitwidth **unsigned** (no
carry → no `nuw` violation), but it gives **no** bound on the *signed*
sum. Adding `(B & ~C1)` to `A + C1` can flip the sign bit and signed-
overflow, even when the original `A + C1` itself did not.

In short:
- `add nsw A, C1` true ⇒ `A + C1` fits in `[-2^(bw-1), 2^(bw-1)-1]`.
- `or disjoint` non-poison ⇒ `(A+C1) | (B & ~C1)` fits in `[0, 2^bw-1]`,
  i.e. unsigned only.
- `A + (B|C1)` true value = unsigned-OK, but **may be ≥ 2^(bw-1)**, so
  `add nsw` is **not** valid.

`NUWOut` is sound (the `NUW && match(NUWAdd)` path is correct because
NUW is genuinely preserved). `NSWOut` is not.

## Repro

`/tmp/w665_hunt/t14b.ll`:

```llvm
define i8 @bug_or_disjoint(i8 %a, i8 %b_in) {
  %lhs = add nsw i8 %a, 5
  %rhs = and i8 %b_in, 250        ; ~5 in i8
  %r   = or disjoint i8 %lhs, %rhs
  ret i8 %r
}
```

### opt output (`/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -passes=instcombine -S`)

```llvm
define i8 @bug_or_disjoint(i8 %a, i8 %b_in) {
  %1 = or i8 %b_in, 5
  %r = add nsw i8 %a, %1          ; <-- NSW NOT justified
  ret i8 %r
}
```

### Diff (concrete inputs: `a = 100`, `b_in = 130`)

| Step | Original IR | After instcombine |
| --- | --- | --- |
| `lhs` | `add nsw i8 100, 5 = 105` (no signed overflow: 105 ≤ 127) | — |
| `rhs` | `and i8 130, 250 = 130` | — |
| disjoint? | `105 & 130 = 0b01101001 & 0b10000010 = 0` ✓ | — |
| `r` | `or disjoint 105, 130 = 235 = -21 (i8)` | `or 130, 5 = 135` |
| `add` (target only) | — | `add nsw 100, 135 = 235` math → **signed overflow → POISON** |
| return | `-21` (concrete i8) | **`poison`** |

The original program always returns `-21` for these inputs; the
optimized program returns `poison`. Introducing poison where the
source is concrete is a refinement violation = miscompile.

## Why the disjoint constraint is satisfied yet target overflows

The disjoint OR proves the two summands are bitwise non-overlapping
*as i8 values*, so their sum equals the OR and fits in `[0, 255]`. It
does **not** prove they fit signed: 235 fits an i8 (as `-21`), but the
*mathematical* sum `100 + 135 = 235` is ≥ 128, so `add nsw` is poison.

## Fix sketch

Drop NSW. The disjoint-or guarantees `nuw`, not `nsw`. The simplest
sound change:
```cpp
bool NSWOut = false;  // NSW from outer disjoint-or is unprovable here
bool NUWOut = NUW;    // disjoint-or already implies the new sum is
                      // bit-disjoint and fits unsigned
```
(Alternatively, keep NSW only when both `(A+C1)` is `nsw` *and* the
high bits of `(B & ~C1)` are known zero — i.e. NSW must additionally
imply the new sum can’t cross the sign bit. Conservative: just drop
NSW.)

Note `NUWOut` is currently `NUW && match(LHS, m_NUWAdd(...))`. For
the `or disjoint` caller this is over-conservative (NUW is always
provable from the disjoint property); a separate missed-opt
opportunity, but not a miscompile.

## Reachability

The buggy NSW=true is reached only when the caller passes `NSW=true`.
Only `visitOr` (the `or disjoint` path) does this from the AndOrXor
focus file. The `visitAdd` caller passes `I.hasNoSignedWrap()`, which
is honest about the *outer* add’s NSW flag. So the bug is specific to
the `or disjoint → add` rewrite.

## Time-box note

Other focus targets (`foldLogicCastConstant` chained icmp / nneg
propagation; `or X, -1 → -1` with poison X; `xor X, X → 0` with
poison X) were each examined and analyzed; no exploitable defect was
found in those for this commit.
