# m086: `SimplifyDemandedBitsForTargetNode` over-claims known bits for `amdgcn.set_inactive`

*Discovery method: code inspection.* Sibling shape to `m076` -- a
target-node "knownbits" hook lying about a node that can actually have
bit values from a second operand the hook never visits.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:5838-5846`
(`AMDGPUTargetLowering::SimplifyDemandedBitsForTargetNode`):

```cpp
case Intrinsic::amdgcn_readfirstlane:
case Intrinsic::amdgcn_readlane:
case Intrinsic::amdgcn_set_inactive:
case Intrinsic::amdgcn_wwm: {
  if (SimplifyDemandedBits(Op.getOperand(1), OriginalDemandedBits,
                           OriginalDemandedElts, Known, TLO, Depth + 1))
    return true;
  break;
}
```

For `readfirstlane` / `readlane` / `wwm` this is correct: the result is
some lane's `Op.getOperand(1)` value, so `Known(result) ⊆ Known(operand1)`.

For `amdgcn.set_inactive(value, inactive_value)` the result is `value`
in **active** lanes and `inactive_value` in **inactive** lanes -- the
correct `Known` is the intersection `Known(value) ∩ Known(inactive)`.
The hook populates `Known` only from `Op.getOperand(1) = value`, never
visiting `Op.getOperand(2) = inactive_value`.

When `value` is a constant, every bit of `Known(value)` is determined,
and the generic `SimplifyDemandedBits` framework
(`TargetLowering.cpp` ~line 3119, the
`DemandedBits.isSubsetOf(Known.Zero | Known.One)` arm) constant-folds
the entire intrinsic to that constant -- silently dropping the
`inactive_value` operand.

(`amdgcn_set_inactive_chain_arg` -- same operand layout -- is missing
from this switch too, but is at least not over-claiming on the
inactive operand by virtue of falling through.)

## Reproducer

`reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) #0 {
entry:
  %wi  = call i32 @llvm.amdgcn.workitem.id.x()
  %is0 = icmp eq i32 %wi, 0
  br i1 %is0, label %do_si, label %ret

do_si:
  ; set_inactive(0xAAAAAAAA, 0x55555555) -- value AAAA..., inactive 5555...
  %v  = call i32 @llvm.amdgcn.set.inactive.i32(i32 -1431655766, i32 1431655765)
  %ze = and i32 %v, 65535
  %w  = call i32 @llvm.amdgcn.strict.wwm.i32(i32 %ze)
  %x  = call i32 @llvm.amdgcn.readlane(i32 %w, i32 1)
  store i32 %x, ptr addrspace(1) %out, align 4
  br label %ret

ret:
  ret void
}
```

`run_ll_reproducer.sh` cannot drive a round-trip runtime check
because the harness launches `block_dim=256` -- every wave runs with
`EXEC=all-ones`, so creating inactive lanes requires divergent control
flow that then has to be carefully wrapped to keep
`strict.wwm`/`readlane`-style cross-lane code well-defined.  The asm
divergence below is the definitive proof.

## Asm-level demonstration (gfx950)

```bash
clang -O0 -target amdgcn-amd-amdhsa -mcpu=gfx950 -nogpulib -S \
    -x ir amdgpu/known-miscompiles/m086-set-inactive-known-bits-overclaim/reduced.ll \
    -o /tmp/o0.s
clang -O2 -target amdgcn-amd-amdhsa -mcpu=gfx950 -nogpulib -S \
    -x ir amdgpu/known-miscompiles/m086-set-inactive-known-bits-overclaim/reduced.ll \
    -o /tmp/o2.s
```

`-O0` (correct):

```asm
s_mov_b32 s4, 0x55555555            ; inactive_value materialized
s_mov_b32 s2, 0xaaaaaaaa            ; value materialized
v_cndmask_b32_e64 v1, v1, v0, s[2:3]  ; select inactive_value / value per lane
```

`-O2` (BUG):

```asm
s_mov_b32 s2, 0xaaaa                ; just the AND-masked constant 0xAAAA
                                    ; no inactive_value, no cndmask
```

At `-O2` the entire `set_inactive` SDNode is constant-folded to
`0xAAAAAAAA`, the `& 0xFFFF` is then folded to `0xAAAA`, and the
`strict.wwm` / `readlane` chain drops away.  Any lane that was meant
to receive `inactive_value` ends up seeing `0xAAAA` instead of the
correct `0x5555`.

## How a fix should look

Split `set_inactive` (and `set_inactive_chain_arg`) out of the joint
case and intersect both operands' known bits:

```cpp
case Intrinsic::amdgcn_set_inactive:
case Intrinsic::amdgcn_set_inactive_chain_arg: {
  KnownBits KnownValue, KnownInactive;
  if (SimplifyDemandedBits(Op.getOperand(1), OriginalDemandedBits,
                           OriginalDemandedElts, KnownValue, TLO, Depth + 1))
    return true;
  if (SimplifyDemandedBits(Op.getOperand(2), OriginalDemandedBits,
                           OriginalDemandedElts, KnownInactive, TLO, Depth + 1))
    return true;
  Known = KnownValue.intersectWith(KnownInactive);
  break;
}
```

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (O2 asm has no cndmask, no `0x55555555`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same code path; reproduces. |

`SimplifyDemandedBitsForTargetNode` is part of the default SDAG
combiner, so this is a default-pipeline `clang -O2` miscompile of any
IR-level use of `set_inactive` with a constant `value` operand whose
demanded bits don't fully constrain `inactive_value`.
