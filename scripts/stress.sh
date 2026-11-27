#!/usr/bin/env bash
# Run a test binary under load until it fails: each run beside a second copy of the whole binary.
# Usage: scripts/stress.sh <test binary> [test name] [runs, default 10]
# Logs go to tmp/stress/<binary>-<n>.log; the script exits 1 at the first failure.
set -euo pipefail

bin=${1:?usage: scripts/stress.sh <test binary> [test name] [runs]}
test=${2:-}
runs=${3:-10}
cd "$(dirname "$0")/.."

# The test binary needs CARGO_BIN_EXE_<name> when it runs outside cargo.
artifacts=$(cargo test --test "$bin" --no-run --message-format=json)
exe=$(jq -r --arg b "$bin" 'select(.reason == "compiler-artifact" and .target.name == $b
  and (.target.kind | index("test")) and .executable != null) | .executable' <<<"$artifacts" | tail -1)
while read -r name path; do
  export "CARGO_BIN_EXE_$name=$path"
done < <(jq -r 'select(.reason == "compiler-artifact" and (.target.kind | index("bin"))
  and .executable != null) | "\(.target.name) \(.executable)"' <<<"$artifacts")
export CARGO_MANIFEST_DIR=$PWD

mkdir -p tmp/stress
for n in $(seq 1 "$runs"); do
  log=tmp/stress/$bin-$n.log
  "$exe" ${test:+--skip "$test"} >"$log.load" 2>&1 &
  load=$!
  if "$exe" ${test:+"$test" --exact} >"$log" 2>&1; then status=0; else status=$?; fi
  wait "$load" || true
  if [ "$status" -ne 0 ]; then
    echo "run $n of $runs failed: $log"
    exit 1
  fi
  echo "run $n of $runs passed"
done
