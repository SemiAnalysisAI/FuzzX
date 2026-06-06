# 039 - PHI nonzero canonicalization ignores `icmp samesign`

Component: `llvm/lib/Transforms/InstCombine/InstCombinePHI.cpp:1467`

The "PHI only compared with zero" fold replaces incoming values that are
known nonzero with an arbitrary nonzero constant. That is valid for plain
`icmp eq/ne %phi, 0`, but not for `icmp samesign`, because changing a
known-nonzero value's sign can make the comparison poison.

InstCombine rewrites:

```llvm
%v = phi i32 [ -1, %neg ], [ 1, %pos ], [ %y, %unk ]
%cmp1 = icmp samesign ne i32 %v, 0
```

to:

```llvm
%v = phi i32 [ -1, %neg ], [ -1, %pos ], [ %y, %unk ]
%cmp1 = icmp samesign ne i32 %v, 0
```

Witness: on the `%sel == 1` path, the source has `%v = 1`, so `1` and `0`
are both non-negative and `icmp samesign ne i32 1, 0` is defined `true`.
After InstCombine, the same path has `%v = -1`, so `-1` and `0` have
different signs and the `samesign` comparison produces poison.

This is an ordinary integer-target miscompile. It does not depend on pointer
representation details, fast-math flags, or strictfp.

Verifier: Anscombe the 2nd (019e9977-7593-71a1-8143-dba1f7b0c6f1) returned
YES.
