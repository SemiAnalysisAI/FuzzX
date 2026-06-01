# 018 — replaceImageHandle: image/texture/surface handle reaching the access via an unhandled def (e.g. select -> SELP) hits default llvm_unreachable

- **Kind:** crash (abort/UB)
- **Reachable via:** default llc, CUDA tex/surf
- **Component:** NVPTXReplaceImageHandles.cpp 1800-1836 (crash at 1834-1835)  (round-3 area `T18-image-handles`)
- **Candidate id:** r3_03

## Summary

a texture/surface handle produced by `select` (→ SELP) hits `llvm_unreachable` in NVPTXReplaceImageHandles

## Mechanism / root cause

replaceImageHandle() finds the def of the handle vreg via `MachineInstr &TexHandleDef = *MRI.getVRegDef(Op.getReg());` and switches only on four opcodes: LD_i64, texsurf_handles, nvvm_move_i64, and TargetOpcode::COPY. Everything else falls into `default: llvm_unreachable("Unknown instruction operating on handle");` (line 1834-1835). The texture/surface handle is a plain i64 SSA value, and the intrinsics (e.g. llvm.nvvm.tex.unified.1d.v4f32.s32, llvm.nvvm.suld.1d.i32.trap) accept any i64. If the i64 handle is produced by a select (selecting between two texsurf.handle.internal results, or any other i64), the reaching def after ISel is a SELP_b64 (not LD_i64/texsurf_handles/move/COPY), so the switch reaches the unreachable. This is valid, non-UB IR: `%h = select i1 %c, i64 %h0, i64 %h1` then used as the texture/surface handle. The same firing path also leaks via the recursive COPY/move case which forwards getOperand(1) to a SELP def. Because this is llvm_unreachable (not report_fatal_error), in a release/NDEBUG build it lowers to __builtin_unreachable() (UB / corrupt codegen), not a graceful diagnostic; in an assertions build it is a hard abort. Either way it is a backend crash reachable from well-defined input rather than a 'cannot select'/unsupported-op diagnostic.

## Trigger

A CUDA-target NVPTX kernel that forms a texture or surface handle (i64) with a `select` and passes it to a tex.unified / suld / sust intrinsic. The select lowers to SELP_b64, whose opcode is not in replaceImageHandle's switch.

## Reproducer

```
target triple = "nvptx64-unknown-cuda"

declare { float, float, float, float } @llvm.nvvm.tex.unified.1d.v4f32.s32(i64, i32)
declare i64 @llvm.nvvm.texsurf.handle.internal.p1(ptr addrspace(1))

@tex0 = internal addrspace(1) global i64 0, align 8
@tex1 = internal addrspace(1) global i64 0, align 8

define ptx_kernel void @sel(ptr %red, i32 %idx, i1 %c) {
entry:
  %h0 = tail call i64 @llvm.nvvm.texsurf.handle.internal.p1(ptr addrspace(1) @tex0)
  %h1 = tail call i64 @llvm.nvvm.texsurf.handle.internal.p1(ptr addrspace(1) @tex1)
  %h = select i1 %c, i64 %h0, i64 %h1
  %val = tail call { float, float, float, float } @llvm.nvvm.tex.unified.1d.v4f32.s32(i64 %h, i32 %idx)
  %ret = extractvalue { float, float, float, float } %val, 0
  store float %ret, ptr %red
  ret void
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_30 -o - repro.ll`

## Observed (wrong) output

```
Unknown instruction operating on handle
UNREACHABLE executed at /Users/justinlebar/code/llvm2/llvm/lib/Target/NVPTX/NVPTXReplaceImageHandles.cpp:1835!
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
0.	Program arguments: /Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64 -mcpu=sm_30 -o -
1.	Running pass 'Function Pass Manager' on module '<stdin>'.
2.	Running pass 'NVPTX Replace Image Handles' on function '@sel'
 #7 ... llvm::NVPTXMachineFunctionInfo::getImageHandleSymbolIndex(llvm::StringRef)
 #8 ... (anonymous namespace)::NVPTXReplaceImageHandles::runOnMachineFunction(llvm::MachineFunction&)
 #9 ... llvm::MachineFunctionPass::runOnFunction(llvm::Function&)

(Control: identical kernel without the select compiles cleanly to:
  tex.1d.v4.f32.s32 	{%r2, %r3, %r4, %r5}, [tex0, {%r1}];)
```

## Expected

A correct compiler must not hit llvm_unreachable on this valid IR. The handle reaching the tex.unified op is defined by SELP_b64 (from the i64 select); replaceImageHandle's switch should either (a) handle the SELP/select case by recursing into both source operands of the select and replacing each handle reference (and, if both resolve to compile-time texref symbols/globals, narrow accordingly or keep the runtime select of two register handles), or (b) at minimum emit a proper, recoverable diagnostic (report_fatal_error "unsupported texture/surface handle" / a 'cannot select'-style error) instead of an llvm_unreachable that aborts in assertions builds and becomes UB (__builtin_unreachable) in NDEBUG. The fully correct behavior is to support the runtime-selected handle and emit a register-form tex instruction whose handle operand is the SELP result (PTX texture instructions can take a register operand for the texref), e.g. tex.1d.v4.f32.s32 {...}, [%rd_handle, {%r1}];

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 0.97).

> CONFIRMED backend crash on valid, verifier-clean IR.

Mechanism (source-verified at NVPTXReplaceImageHandles.cpp:1800-1836): replaceImageHandle() finds the def of the texture/surface handle vreg via MRI.getVRegDef and switches only on four opcodes: NVPTX::LD_i64, NVPTX::texsurf_handles, NVPTX::nvvm_move_i64, and TargetOpcode::COPY. Anything else falls into `default: llvm_unreachable("Unknown instruction operating on handle")` at line 1834-1835.

The pass (createNVPTXReplaceImageHandlesPass) is added unconditionally in addInstSelector (NVPTXTargetMachine.cpp:413), right after ISel. processInstr (line 1742-1747) sees the tex.unified op (IsTexFlag set) and calls replaceImageHandle on operand 4 (the texref handle).

The texture handle is a plain i64 SSA value and the intrinsic accepts any i64. When the handle is produced by `select i1 %c, i64 %h0, i64 %h1`, ISel lowers it to SELP_b64rr (confirmed via NVPTXInstrInfo.td:925 `(SELP_b64rr $a, $b, $p)` pattern for i64 select). SELP_b64 is not in the switch, so the default unreachable fires.

Empirical proof:
- Reproducer IR passes `opt -passes=verify` (exit 0) — well-formed, no UB, no undef/poison (both handles are defined values from texsurf.handle.internal; select picks one).
- llc (Optimized build WITH assertions) aborts with "Unknown instruction operating on handle / UNREACHABLE executed at NVPTXReplaceImageHandles.cpp:1835" and a s
