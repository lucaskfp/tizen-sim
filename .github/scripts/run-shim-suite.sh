#!/usr/bin/env bash
# Runs the in-page Shim suite on one Version Profile's Engine, with the
# network tests: an Engine's own background requests (model and dictionary
# downloads) would show up in the App's network log, and they differ by
# Engine (#61). On failure, names the Version Profile and each failing test
# as GitHub annotations and in the job summary.
#
# Usage: run-shim-suite.sh <version-profile>
set -uo pipefail

profile="$1"
log="$(mktemp)"

TIZEN_SIM_TEST_PROFILE="$profile" cargo test -p tizen-sim-core --no-fail-fast \
  --test shims --test webapis --test remote_keys --test screen --test network 2>&1 | tee "$log"
status=${PIPESTATUS[0]}
[ "$status" -eq 0 ] && exit 0

# `binary::test` for each failing test: cargo announces each test binary
# ("Running tests/shims.rs (...)"), and libtest prints a failing test's
# output under "---- <test> stdout ----". Colour codes are stripped first.
failures=$(sed -E 's/\x1b\[[0-9;]*m//g' "$log" | awk '
  /^ *Running tests[\\\/]/ { sub(/^ *Running tests[\\\/]/, ""); sub(/\.rs.*/, ""); binary = $0 }
  /^---- .+ stdout ----/ { print binary "::" $2 }
')

summary="${GITHUB_STEP_SUMMARY:-/dev/null}"
echo "### Shim suite failed on Tizen $profile" >> "$summary"
if [ -z "$failures" ]; then
  echo "::error title=Shim suite failed on Tizen $profile::no failing test was reported (build error or crash); see the log"
  echo "No failing test was reported (build error or crash); see the log." >> "$summary"
else
  while read -r test; do
    echo "::error title=Shim suite failed on Tizen $profile::$test"
    echo "- \`$test\`" >> "$summary"
  done <<< "$failures"
fi
exit "$status"
