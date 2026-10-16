#!/usr/bin/env bash
# Prints the GUI updater's manifest, latest.json, for release <tag> of
# <repo> from the `.sig` files `tauri build` wrote next to each installer,
# all in <dir>. The GUI looks itself up as `<os>-<arch>-<installer>` first,
# so each installer updates to its own kind, then as `<os>-<arch>`. That
# fallback is only given on Windows, where the updater runs whichever
# installer it gets: a `.deb` install would hand an AppImage to dpkg.
#
# Usage: updater-manifest.sh <tag> <repo> <dir> [notes-file]
set -euo pipefail

tag=$1 repo=$2 dir=$3 notes=${4:-}
platforms='{}'

add() { # <key> <sig file>
  local file
  file=$(basename "$2" .sig)
  platforms=$(jq --arg key "$1" --arg url "https://github.com/$repo/releases/download/$tag/$file" \
    --rawfile signature "$2" '.[$key] = {url: $url, signature: $signature}' <<<"$platforms")
}

shopt -s nullglob
for sig in "$dir"/*.sig; do
  case $sig in
    *-setup.exe.sig) add windows-x86_64-nsis "$sig"; add windows-x86_64 "$sig" ;;
    *.msi.sig) add windows-x86_64-msi "$sig" ;;
    *.AppImage.sig) add linux-x86_64-appimage "$sig" ;;
    *.deb.sig) add linux-x86_64-deb "$sig" ;;
  esac
done

if [ "$platforms" = '{}' ]; then
  echo "no installer signatures in $dir" >&2
  exit 1
fi

jq -n --arg version "${tag#v}" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg notes "$([ -n "$notes" ] && cat "$notes")" --argjson platforms "$platforms" \
  '{version: $version, notes: $notes, pub_date: $date, platforms: $platforms}'
