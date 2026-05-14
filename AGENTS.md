# Repository Instructions

## ptxas Miscompile Workflow

When investigating, reducing, or writing standalone reproducers for ptxas
miscompiles, always verify the final reduced testcase against the latest
available CUDA Toolkit ptxas, not just the system-default ptxas. Check NVIDIA's
current CUDA download/archive pages at task time, use the newest available
ptxas, and record the exact ptxas version/build in the repro notes.

If a newer ptxas is not already installed locally, download or extract the
newest available toolkit/compiler package when feasible and run the standalone
reproducer or reduced PTX against that binary explicitly via `PTXAS=/path/to/ptxas`.
