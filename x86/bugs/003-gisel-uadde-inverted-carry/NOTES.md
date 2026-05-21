# 003 — GISel `selectUAddSub` materializes carry-in with `CMP r, 1`, inverting CF

Component: X86 GISel (X86InstructionSelector)

## Source

`llvm/lib/Target/X86/GISel/X86InstructionSelector.cpp`,
`X86InstructionSelectorImpl::selectUAddSub` around lines 1262–1293
(`G_UADDE` / `G_USUBE` carry-in path).

When the carry-in to `G_UADDE` (or `G_USUBE`) is a register holding the prior
SETcc byte (0 or 1), the selector emits:

```
CMP8ri  carryInReg, 1
ADC32rr ...                ; or SBB / 16 / 64 variant
```

`CMP r, 1` computes `r - 1` and sets `CF` iff `r < 1` unsigned, i.e.
`CF = (r == 0)`. That's the *inverse* of the intended carry:

| carryIn byte | desired CF | emitted CF after `CMP r, 1` |
| ---: | ---: | ---: |
| 1 (carry happened) | 1 | 0 |
| 0 (no carry)       | 0 | 1 |

So the subsequent `ADC` adds the WRONG carry bit. The corresponding
`G_USUBE` path emits `SBB` with the same inverted CF — equally broken.

## Runtime demonstration (x86_64, i128 add)

```ll
define i128 @add128(i128 %a, i128 %b) {
  %r = add i128 %a, %b
  ret i128 %r
}
```

`llc -O2 -mtriple=x86_64-linux-gnu -global-isel` emits:

```
movq    %rdx, %rax
addq    %rdi, %rax       ; low add, sets CF
setb    %dl              ; dl = 0/1
cmpb    $1, %dl          ; CF = (dl < 1) = (dl == 0)   <-- INVERTED
adcq    %rsi, %rcx       ; rcx += rsi + (inverted CF)
movq    %rcx, %rdx
retq
```

Without `-global-isel`, the default SDAG path correctly emits
`addq ; adcq` with no intervening SETB/CMP.

`runner.c` calls `add128(0xFFFFFFFF_FFFFFFFF, 1)`. The correct result is
`(hi=1, lo=0)`; the GISel-compiled binary returns `(hi=0, lo=0)` — a
1-bit error in the high half because the carry was eaten.

Output from `./cmd.sh`:

```
add128(0xFFFF_FFFF_FFFF_FFFF, 1) = (hi=0x0000000000000000 lo=0x0000000000000000)
expected:                       (hi=0x0000000000000001 lo=0x0000000000000000)
FAIL — multi-word add carried wrong amount (GISel uadde carry-in bug)
```

## Why it slipped past the test suite

`llvm/test/CodeGen/X86/GlobalISel/select-add-x32.mir` literally pins the
incorrect `CMP %carry, 1 ; ADC` sequence in its `CHECK` lines, so the test
suite asserts the buggy output is preserved.

## Fix sketch

Replace `CMP r, 1` (which sets CF = !r-as-bool) with a sequence that sets
`CF = r-as-bool`. Two cheap options:

- `ADD8ri  carryInReg, -1`  (equivalently `0xFF`) — CF set iff input is `0`. Still wrong direction, but: `ADD r, 0xff` sets CF iff `r != 0` is FALSE → also wrong, ignore.
- `NEG8r   carryInReg`      — `NEG` sets CF iff the operand is non-zero. ✓
- `SUB8ri  carryInReg, 1` then negate via condition swap — fragile.

The canonical fix is `NEG8r carryInReg` (or `TEST + JZ`, but `NEG` is cheaper)
just before `ADC`/`SBB`. (`NEG r; ADC ...` is the same pattern SDAG uses for
the same pattern via `X86::COPY_TO_REGCLASS` of an `X86::SETCCr COND_B`.)

## Scope

Affects any `-global-isel` x86 build that ends up with chained
G_UADDE/G_UADDO/G_USUBE/G_USUBO — typically `add` / `sub` on i64 (i386
target) or i128 / wider (x86_64 target).

## Files
- `repro.ll` — IR
- `runner.c` — driver that triggers the wrong carry
- `cmd.sh`   — builds & runs; non-zero exit means bug reproduced
