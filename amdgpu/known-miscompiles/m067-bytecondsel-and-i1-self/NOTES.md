# m067: `select i1 (and i1 X, X) c, 0` at `-O0` returns the wrong byte after a bytecondsel chain

Found while fuzzing LLVM HEAD with llvm/llvm-project#196418,
llvm/llvm-project#198412, llvm/llvm-project#198491, llvm/llvm-project#198508,
and llvm/llvm-project#198556 applied.  The original oracle finding was:

```text
kind=oracle
index=0
input=0x0
o0=0xCE
o2=0x59
expected=0x59
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m067-bytecondsel-and-i1-self/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches and on ROCm HEAD:

```text
input=0x00000000
O0=0x000000ce
O2=0x00000059
mismatch=true
```

ROCm 7.2.3 passes (`O0=O2=0x59`), so this is a HEAD regression introduced
after the 7.2.3 release.

## Reduction

`llvm-reduce` reduced the 348-line generated IR to 106 lines.  The reduced
kernel computes a small chain of `bytecarry` + `vecbytegather` ops that
produce the `i32` value `0xfb03`, extracts byte 0 (`a.byte = 3`) and byte 1
(`c.byte = 251`), then runs:

```llvm
%cmp.first = icmp ult i32 %a.byte, 0          ; always false
%comb.mask = and i1 %cmp.first, %cmp.first    ; same as %cmp.first
%sel.three = select i1 %comb.mask, i32 %c.byte, i32 0
%key.xor   = xor i32 67, %sel.three            ; 67 ^ 0 = 67
%key.next  = add i32 %key.xor, 22              ; 67 + 22 = 89 = 0x59
store i32 %key.next, ptr addrspace(1) %out
```

For input zero the select condition is provably false, so the store value
should be `0x59`.  `-O2` (and ROCm 7.2.3 `-O0`) match; LLVM HEAD and ROCm
HEAD `-O0` store `0xCE` (= 206), which is `0x59 + 117` — consistent with
`%sel.three` being evaluated to a non-zero value (likely `%c.byte = 251`,
since `(67 ^ 251) + 22 = 0xCE`) when it should be 0.

## Root Cause Notes

The triggering pattern is `select i1 (and i1 X, X) c, 0`.  Semantically the
`and i1 X, X` is just `X`, so the select reduces to `X ? c : 0` and for the
false case produces `0`.  The `-O0` lowering for this exact shape — where
the select condition is an `and i1` of a value with itself, fed by an
`icmp ult i32 X, 0` (i.e. an unsigned compare against zero that is always
false) — appears to drop or misfold the condition, evaluating the select as
if the condition were true.

This is consistent with the bytecondsel idiom's `case 3` branch in
`emitRandomBytewiseConditionalSelectFoldIdiom`:

```cpp
Combined = B.CreateAnd(B.CreateXor(FirstCmp, SecondCmp,
                                   Twine(NamePrefix) + ".comb.xor2"),
                       FirstCmp, Twine(NamePrefix) + ".comb.mask");
```

When `SecondCmp == FirstCmp` (which happens when the reducer collapses both
into the same value), the `xor` is identically false and the outer `and`
becomes `and i1 FirstCmp, FirstCmp`, which is what reaches the select.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: `O0=O2=0x59`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#196418, llvm/llvm-project#198412, llvm/llvm-project#198491, llvm/llvm-project#198508, llvm/llvm-project#198556 applied locally | Reproduces: `O0=0xCE`, `O2=0x59`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with the same five PR patches applied locally | Reproduces: `O0=0xCE`, `O2=0x59`. |

Original fuzzer input SHA-1:

```text
e8a60e7795dd21680ba80f22203bc2991b66436c
```

Reduced reproducer SHA-1:

```text
d5e24c4c46b6cf2791ffcc556a1ce8aaf69b88cd
```

## Fuzzer Follow-Up

The fuzzer now rejects final stores whose value depends on generated
`bytecondsel` idiom output by default.  Set
`FUZZX_ALLOW_M067_BYTECONDSEL_SELF_AND=1` to re-enable this bug class.
