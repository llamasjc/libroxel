#!/usr/bin/env bash
# bench-compare.sh <base commit>: builds the benchmarks for the base commit
# and this one, runs them in turn on this computer, and fails when this
# commit is much slower (see crates/playlibre-tests/src/bin/bench.rs).
#
# The base is benchmarked with this commit's benchmark code, so a new or
# changed workload is timed the same way on both sides. If that code
# doesn't build against the base (the engine's API changed), the
# comparison is skipped.
set -euo pipefail
base="$1"
threshold="${BENCH_THRESHOLD:-1.3}"
out="$(mktemp -d)"
summary="${GITHUB_STEP_SUMMARY:-/dev/stdout}"
target="$PWD/target"
# Both sides play this commit's corpus.
export PLAYLIBRE_CORPUS="$PWD/crates/playlibre-tests/corpus"

cargo build --locked -p playlibre-tests --bin bench
cp "$target/debug/bench" "$out/bench-new"

git fetch --no-tags --depth=1 origin "$base"
rm -rf "$out/base"
git worktree add --detach "$out/base" "$base"
rm -rf "$out/base/crates/playlibre-tests"
cp -r crates/playlibre-tests "$out/base/crates/playlibre-tests"
if ! (cd "$out/base" && CARGO_TARGET_DIR="$target" cargo build -p playlibre-tests --bin bench); then
  echo "::notice::The benchmarks don't build against ${base::7}, so there is nothing to compare with."
  echo "Benchmarks: no comparison (they don't build against ${base::7})." >> "$summary"
  "$out/bench-new" run --repeat 1
  exit 0
fi
cp "$target/debug/bench" "$out/bench-old"
git worktree remove --force "$out/base"

# Old, new, old, new: a runner that slows down partway hits both sides.
for round in 1 2; do
  for side in old new; do
    echo "== $side, round $round"
    "$out/bench-$side" run --repeat 2 --out "$out/$side-$round.json"
  done
done

{
  echo "### Benchmarks against ${base::7}"
  echo
} >> "$summary"
set +e
"$out/bench-new" compare --old "$out/old-1.json,$out/old-2.json" --new "$out/new-1.json,$out/new-2.json" \
  --threshold "$threshold" | tee -a "$summary"
code=${PIPESTATUS[0]}
set -e
if [[ $code -ne 0 ]]; then
  echo "::error::A benchmark is more than ${threshold}x slower than ${base::7}. If that's expected, say why in the commit message."
fi
exit "$code"
