#!/usr/bin/env bash
# Starts the built GUI on the current X display and checks that its window
# comes up and that it keeps running, then closes it.
#
# Usage: gui-starts.sh <path/to/tizen-sim-gui>
set -uo pipefail

gui="$1"
log="$(mktemp)"
# On the X display, where its window is looked for, even under Wayland.
GDK_BACKEND=x11 "$gui" > "$log" 2>&1 &
pid=$!

# Its window, by title, within 30s.
shown=""
for _ in $(seq 60); do
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "::error title=GUI smoke test::the GUI exited at startup"
    cat "$log"
    exit 1
  fi
  if xwininfo -root -tree | grep -q '"tizen-sim"'; then
    shown=1
    break
  fi
  sleep 0.5
done
if [ -z "$shown" ]; then
  echo "::error title=GUI smoke test::the GUI window never appeared"
  cat "$log"
  kill "$pid"
  exit 1
fi

# Still up a few seconds later: the frontend loaded without bringing it down.
sleep 5
if ! kill -0 "$pid" 2>/dev/null; then
  echo "::error title=GUI smoke test::the GUI exited after its window appeared"
  cat "$log"
  exit 1
fi
kill "$pid"
wait "$pid" 2>/dev/null
cat "$log"
echo "The GUI started and showed its window."
