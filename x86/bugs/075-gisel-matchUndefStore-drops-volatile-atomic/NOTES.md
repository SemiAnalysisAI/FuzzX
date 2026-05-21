# w47 — GISel `matchUndefStore` erases volatile/atomic stores

**Component:** `llvm/lib/CodeGen/GlobalISel/CombinerHelper.cpp` (`matchUndefStore`)
**Combine rule:** `erase_undef_store` in `llvm/include/llvm/Target/GlobalISel/Combine.td`
**Triggers via:** `-global-isel` legalizer/combiner pipeline on x86_64.
**Status:** miscompile, confirmed in asm.

## Root cause

`CombinerHelper::matchUndefStore` returns `true` whenever the value operand of a
`G_STORE` is `G_IMPLICIT_DEF`, with no other gating:

```cpp
bool CombinerHelper::matchUndefStore(MachineInstr &MI) const {
  assert(MI.getOpcode() == TargetOpcode::G_STORE);
  return getOpcodeDef(TargetOpcode::G_IMPLICIT_DEF, MI.getOperand(0).getReg(),
                      MRI);
}
```

The single user is the `erase_undef_store` `GICombineRule`
(Combine.td:715-720), whose apply is `Helper.eraseInst(*${root})` — unconditional
erase of the G_STORE.

Neither the matcher nor the apply checks `isVolatile()` / `isAtomic()` on the
G_STORE's MachineMemOperand. A `store volatile i32 undef, ptr %p` (which the
language reference requires to actually perform the write, with some
implementation-defined bit pattern) is silently deleted. Likewise
`store atomic i32 undef, ptr %p seq_cst` — and an atomic store has visible
synchronization semantics regardless of value.

This is the same class of bug as bugs/015, 017, 041 on the DAG/codegen side, but
inside the GISel combiner.

## Reproducer

`/tmp/w47-undef-store.ll`:

```ll
target triple = "x86_64-unknown-linux-gnu"

define void @vstore_undef(ptr %p) {
  store volatile i32 undef, ptr %p, align 4
  ret void
}

define void @astore_undef(ptr %p) {
  store atomic i32 undef, ptr %p seq_cst, align 4
  ret void
}
```

Command:
```
llc -mtriple=x86_64-unknown-linux-gnu -global-isel -global-isel-abort=2 \
    /tmp/w47-undef-store.ll -o -
```

### Actual output (GISel)

```
vstore_undef:
	retq
astore_undef:
	retq
```

### Expected output (DAG ISel emits, same llc without `-global-isel`)

```
vstore_undef:
	movl	%eax, (%rdi)        # volatile store materialised
	retq
astore_undef:
	xchgl	%eax, (%rdi)        # seq_cst atomic store materialised
	retq
```

`stop-after=legalizer` shows the G_STORE is already gone after the GISel
combiner runs. The functions are reduced to a single `RET 0`.

## Severity

- The volatile-store fold drops an externally-visible side effect (MMIO, JIT
  patching, signal-handler-visible writes).
- The atomic-store fold drops a memory ordering fence; downstream loads may
  observe stale state.
- Worse than the DAG analogues because GISel is documented as the future
  default; getting `-global-isel` enabled on x86 with this hole means dropped
  volatile and atomic stores throughout.

## Suggested fix

In `matchUndefStore`, additionally require the G_STORE's MMO to be neither
volatile nor atomic before reporting a match:

```cpp
bool CombinerHelper::matchUndefStore(MachineInstr &MI) const {
  auto &Store = cast<GStore>(MI);
  if (Store.isVolatile() || !Store.isUnordered())
    return false;
  return getOpcodeDef(TargetOpcode::G_IMPLICIT_DEF,
                      Store.getValueReg(), MRI);
}
```

(Or, equivalently, guard inside `eraseInst`'s combine-rule apply.)
