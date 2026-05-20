#!/usr/bin/env python3
"""Identify AMDGPU-codegen coverage gaps in a fuzzer worker's runtime counters.

Usage:
  scripts/analyze_coverage_gaps.py BINARY CNTRS_DUMP [--pcs PC_FILE] \
                                   [--sym SYM_FILE] [--top N]

The script reports the AMDGPU-related functions whose sancov 8-bit counter
blocks are cold (== 0) after the run.  Each function is ranked by the number
of cold basic blocks, with the fraction (cold / total) also shown so a small
"never reached" helper isn't drowned out by a big mostly-covered pass.
"""
from __future__ import annotations

import argparse
import collections
import struct
import subprocess
import sys
from pathlib import Path


def section(binary: Path, name: str) -> tuple[int, int, int]:
    """Return (VMA, file_offset, size) for an ELF section."""
    out = subprocess.check_output(["objdump", "-h", str(binary)]).decode()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 7 and parts[1] == name:
            size = int(parts[2], 16)
            vma = int(parts[3], 16)
            foff = int(parts[5], 16)
            return vma, foff, size
    raise SystemExit(f"section {name!r} not found in {binary}")


def load_pcs(binary: Path, sym_path: Path | None) -> list[str]:
    """Return list of length N counters, each entry = function name."""
    vma, foff, size = section(binary, "__sancov_pcs")
    with open(binary, "rb") as f:
        f.seek(foff)
        data = f.read(size)
    raw = struct.unpack(f"<{len(data)//8}Q", data)
    pcs = [raw[i] for i in range(0, len(raw), 2)]

    if sym_path and sym_path.exists():
        funcs: list[str] = []
        with open(sym_path) as f:
            block: list[str] = []
            for line in f:
                line = line.rstrip("\n")
                if line == "":
                    if block:
                        funcs.append(block[0])
                    block = []
                else:
                    block.append(line)
            if block:
                funcs.append(block[0])
        if len(funcs) == len(pcs):
            return funcs
        sys.stderr.write(
            f"warning: cached sym file has {len(funcs)} entries but binary has "
            f"{len(pcs)} PCs; re-symbolising\n"
        )

    pcs_txt = "\n".join(f"0x{p:x}" for p in pcs) + "\n"
    sym_bin = "/opt/rocm-7.1.1/lib/llvm/bin/llvm-symbolizer"
    out = subprocess.run(
        [sym_bin, f"--obj={binary}", "--functions=linkage", "--demangle"],
        input=pcs_txt,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    funcs = []
    block: list[str] = []
    for line in out.splitlines():
        if line == "":
            funcs.append(block[0] if block else "??")
            block = []
        else:
            block.append(line)
    if block:
        funcs.append(block[0])
    if len(funcs) != len(pcs):
        raise SystemExit(
            f"symbolizer produced {len(funcs)} funcs for {len(pcs)} PCs"
        )
    return funcs


def load_cntrs(binary: Path, dump: Path) -> bytes:
    _, _, size = section(binary, "__sancov_cntrs")
    raw = dump.read_bytes()
    if len(raw) != size:
        raise SystemExit(
            f"counter dump has {len(raw)} bytes; expected {size}"
        )
    return raw


AMDGPU_PATTERNS = (
    "amdgpu",
    "AMDGPU",
    "::SI",  # SIRegisterInfo, SIInstrInfo, SIISelLowering, SIFold, ...
    "R600",
    "GCNRegPressure",
    "GCNSchedStrategy",
    "GCNSubtarget",
    "GCNTargetMachine",
    "VOPC",
    "VOP3",
    "WaveTransform",
    "SDWA",
    "SIPreEmitPeephole",
    "SIShrinkInstructions",
    "SILoadStoreOpt",
    "SIPeepholeSDWA",
    "SIInsertWaitcnts",
    "SILowerControlFlow",
    "SILowerSGPRSpills",
    "SIWholeQuadMode",
    "SIOptimizeExecMasking",
    "SIFoldOperands",
    "SIMachineFunctionInfo",
)


def is_amdgpu(func: str) -> bool:
    for p in AMDGPU_PATTERNS:
        if p in func:
            return True
    return False


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("binary", type=Path)
    ap.add_argument("cntrs_dump", type=Path)
    ap.add_argument("--sym", type=Path, default=Path("/tmp/pcs_new.sym"))
    ap.add_argument("--top", type=int, default=40)
    ap.add_argument(
        "--prefix",
        action="append",
        default=None,
        help="extra function-name substring to count as AMDGPU "
        "(can repeat)",
    )
    args = ap.parse_args()

    if args.prefix:
        for p in args.prefix:
            AMDGPU_PATTERNS_LIST = list(AMDGPU_PATTERNS) + [p]
    else:
        AMDGPU_PATTERNS_LIST = list(AMDGPU_PATTERNS)

    def is_match(func: str) -> bool:
        for p in AMDGPU_PATTERNS_LIST:
            if p in func:
                return True
        return False

    print(f"loading PCs+symbols...", file=sys.stderr)
    funcs = load_pcs(args.binary, args.sym)
    print(f"  {len(funcs):,} counter blocks across "
          f"{len(set(funcs)):,} unique funcs", file=sys.stderr)

    print(f"loading counter dump...", file=sys.stderr)
    cntrs = load_cntrs(args.binary, args.cntrs_dump)

    total_blocks = collections.Counter()
    cold_blocks = collections.Counter()
    for i, fn in enumerate(funcs):
        if not is_match(fn):
            continue
        total_blocks[fn] += 1
        if cntrs[i] == 0:
            cold_blocks[fn] += 1

    # Funcs with at least one block but all-cold = fully untested
    fully_cold = [
        (fn, total_blocks[fn])
        for fn in total_blocks
        if cold_blocks[fn] == total_blocks[fn]
    ]
    fully_cold.sort(key=lambda x: -x[1])

    # Partially covered: at least one hot and one cold
    partial = [
        (fn, cold_blocks[fn], total_blocks[fn])
        for fn in total_blocks
        if 0 < cold_blocks[fn] < total_blocks[fn]
    ]
    partial.sort(key=lambda x: -x[1])

    total_amdgpu_funcs = len(total_blocks)
    fully_hot = sum(
        1 for fn in total_blocks if cold_blocks[fn] == 0
    )
    fully_cold_n = len(fully_cold)
    partial_n = len(partial)
    total_amdgpu_blocks = sum(total_blocks.values())
    cold_amdgpu_blocks = sum(cold_blocks.values())

    print()
    print(f"AMDGPU coverage summary:")
    print(f"  funcs total       : {total_amdgpu_funcs:,}")
    print(f"  funcs fully cold  : {fully_cold_n:,}")
    print(f"  funcs fully hot   : {fully_hot:,}")
    print(f"  funcs partial     : {partial_n:,}")
    print(f"  blocks total      : {total_amdgpu_blocks:,}")
    print(f"  blocks cold       : {cold_amdgpu_blocks:,} "
          f"({100*cold_amdgpu_blocks/max(1,total_amdgpu_blocks):.1f}%)")

    print()
    print(f"==== TOP {args.top} fully-cold AMDGPU functions "
          f"(by #blocks) ====")
    for fn, n in fully_cold[: args.top]:
        print(f"  {n:5d}  {fn}")

    print()
    print(f"==== TOP {args.top} partial-coverage AMDGPU funcs "
          f"(by cold-block count) ====")
    for fn, c, t in partial[: args.top]:
        pct = 100 * c / t
        print(f"  {c:5d}/{t:<5d} ({pct:5.1f}%)  {fn}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
