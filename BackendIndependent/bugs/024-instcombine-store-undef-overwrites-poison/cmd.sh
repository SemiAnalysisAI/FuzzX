#!/usr/bin/env bash
set -euo pipefail

OPT=${OPT:-/Users/justinlebar/code/llvm2/build/bin/opt}
"$OPT" -passes=instcombine -S repro.ll -o -
