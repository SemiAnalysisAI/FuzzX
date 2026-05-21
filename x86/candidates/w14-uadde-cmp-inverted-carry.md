# X86 GISel: selectUAddSub inverts carry when materializing CarryIn (CMP r,1 + ADC)

## Location
- `llvm/lib/Target/X86/GISel/X86InstructionSelector.cpp` lines 1262-1293 (`selectUAddSub`, G_UADDE / G_USUBE carry-in path).

## Reasoning
When a G_UADDE/G_USUBE has a carry-in coming from a previous G_UADDE/G_UADDO/G_USUBE/G_USUBO,
the selector emits:

```
CMP8ri  carryInReg, 1     ; sets EFLAGS
ADC32rr ...                ; or SBB / 16 / 64 variant
```

The carry-in register holds the byte result of a prior `SETCCr COND_B`, i.e. it is
`1` when the previous op carried and `0` otherwise. `CMP r, 1` computes `r - 1`
and sets CF iff `r < 1` unsigned, so:

- previous carry = 1 -> CMP result 0, **CF = 0**
- previous carry = 0 -> CMP result 0xff, **CF = 1**

`ADC` then adds CF, i.e. it adds 1 exactly when there was NO carry from the
prior add. The correct sequence should set CF to 1 iff the input byte is
nonzero; e.g. `ADD r, 0xff` or `NEG r` would do that ( both set CF iff
operand != 0), or `CMP r, 0` followed by SETCC reload.

This produces a wrong-code i64 (or i128) ADD/SUB on i386 — and on 64-bit as
soon as the legalizer wide-splits to s64 carries.

## Repro

```
; uadde.ll
define i64 @add(i64 %a, i64 %b) { %r = add i64 %a, %b ret i64 %r }
define i32 @main() {
  %r = call i64 @add(i64 4294967295, i64 1)   ; 0xFFFFFFFF + 1 = 0x1_00000000
  %hi = lshr i64 %r, 32
  %hi32 = trunc i64 %hi to i32
  ret i32 %hi32                                 ; correct = 1
}
```

```
$ llc -mtriple=i386-linux-gnu -global-isel uadde.ll -filetype=obj -o g.o
$ llc -mtriple=i386-linux-gnu             uadde.ll -filetype=obj -o d.o
$ gcc -m32 g.o -o g && ./g; echo $?    # -> 0    (WRONG)
$ gcc -m32 d.o -o d && ./d; echo $?    # -> 1    (correct)
```

GISel asm for `add`:
```
addl  4(%esp), %eax
setb  %cl                ; cl = previous carry (0/1)
cmpb  $1, %cl            ; CF = !cl  -- INVERTED
adcl  8(%esp), %edx
```

## Wrong outcome
Multi-word integer addition/subtraction silently produces a value that differs
from the IR semantics by `+/-1` in the upper word(s). Same bug affects
`G_USUBE` (selects SBB) and 64-bit/128-bit carry-chained sequences on x86_64
once the legalizer narrows wider integers into s64 carries.

## Notes
- The MIR check in `llvm/test/CodeGen/X86/GlobalISel/select-add-x32.mir`
  literally encodes the wrong sequence (`CMP8ri %setcc, 1` + `ADC32rr`),
  which is how the bug slipped past the test suite.
- The constant-carry-in path (`val == 0`) is correct because it just lowers
  to plain ADD/SUB.
