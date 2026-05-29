#!/usr/bin/env bash
OPT=$HOME/code/llvm3/build/bin/opt
"$OPT" -passes=rewrite-statepoints-for-gc -S repro.ll -o /dev/null
