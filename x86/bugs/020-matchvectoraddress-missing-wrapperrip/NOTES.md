# matchVectorAddressRecursively missing X86ISD::WrapperRIP case

**File**: `llvm/lib/Target/X86/X86ISelDAGToDAG.cpp:2937-2972` (function `matchVectorAddressRecursively`)

## Reasoning

`matchAddressRecursively` (lines 2581-2585) handles both `X86ISD::Wrapper`
and `X86ISD::WrapperRIP` by calling `matchWrapper`. The vector-address
variant `matchVectorAddressRecursively` only handles `X86ISD::Wrapper`:

```cpp
case X86ISD::Wrapper:
  if (!matchWrapper(N, AM))
    return false;
  break;
```

When the gather/scatter base pointer is a `WrapperRIP` (which is the
canonical form for accessing globals in 64-bit small/medium code model),
the switch falls through to `matchAddressBase`, which puts the entire
`WrapperRIP` node into `AM.Base_Reg` as a generic register operand. The
result is that the symbol is materialized into a register via a separate
`lea`/`mov`, instead of being folded into the gather/scatter's disp32
(`vpgatherdd  glb(,%xmm0,4), %xmm1`).

The bug is at minimum a missed-optimization; it can also become a
*correctness* issue with TLS Wrappers where `WrapperRIP` carries a TLS
relocation flag that the matchAddressBase path silently drops or that
later layers expect to be in the disp slot rather than the base slot.

## Repro sketch

```ll
@G = global [16 x i32] zeroinitializer
define <4 x i32> @gather_global(<4 x i32> %idx, <4 x i1> %m) {
  %p = getelementptr i32, ptr @G, <4 x i32> %idx
  %v = call <4 x i32> @llvm.masked.gather.v4i32.v4p0(<4 x ptr> %p,
                                                     i32 4, <4 x i1> %m,
                                                     <4 x i32> zeroinitializer)
  ret <4 x i32> %v
}
```

Build with `-mattr=+avx2`. Expected `vpgatherdd G(,%xmm0,4), %xmm1, %xmm2`
in the ideal case, but the matcher only produces `vpgatherdd 0(%rax,%xmm0,4)`
preceded by a `leaq G(%rip), %rax`.

## Wrong outcome

Missed disp32 fold: extra `lea` / `mov` for the symbol. Possible
mis-relocation if a TLS `WrapperRIP` flows into `matchAddressBase`
because the symbolic-displacement bookkeeping
(`AM.GV/CP/JT/BlockAddr/SymbolFlags`) is bypassed.

## Cross-reference

`llvm/test/CodeGen/X86/masked_gather*.ll` exercises gather with globals
but base pointer usually arrives as a `Wrapper` (not `WrapperRIP`) when
the address-computation pass has already split out the index. No test
appears to exercise the WrapperRIP-as-base-of-gather case directly.
