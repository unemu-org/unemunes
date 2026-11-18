#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Fetch the test oracles that are not committed (decision-3) into .oracles/,
# each pinned by URL and SHA-256 in scripts/oracles.manifest.
#
#   scripts/fetch-oracles.sh fetch [SET...]   download what is missing or wrong
#   scripts/fetch-oracles.sh verify [SET...]  offline: check what is present
#   scripts/fetch-oracles.sh list             the sets and their file counts
#
# Without a SET, every set of the manifest. A file whose bytes do not match
# its pin is never kept: fetch fails and removes the download. Tests that need
# a set skip with a message when it is absent.
#
# Exit status: 0 all good; 1 a pin mismatch (download or local copy);
# 2 usage; 3 verify found files missing; 4 a download failed.
#
# Needs bash 3.2, curl, awk and sha256sum or shasum, so it runs the same on
# macOS and Linux without the dev shell.
set -euo pipefail
export LC_ALL=C

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$root/scripts/oracles.manifest"
dest="$root/.oracles"

usage() {
  sed -n '3,9p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$1" | awk '{ print $1 }'
  else
    shasum -a 256 -- "$1" | awk '{ print $1 }'
  fi
}

# The manifest's entries for the selected sets: "set sha256 path url" lines.
entries() {
  awk -v sets="$*" '
    BEGIN { n = split(sets, wanted, " "); for (i = 1; i <= n; i++) want[wanted[i]] = 1 }
    /^[[:space:]]*(#|$)/ { next }
    NF != 4 { printf "malformed manifest line %d\n", NR > "/dev/stderr"; exit 2 }
    n == 0 || ($1 in want) { print; seen[$1] = 1 }
    END { for (s in want) if (!(s in seen)) { printf "unknown set: %s\n", s > "/dev/stderr"; exit 2 } }
  ' "$manifest"
}

cmd=${1:-}
[[ -n $cmd ]] || usage
shift

case "$cmd" in
  list)
    awk '/^[[:space:]]*(#|$)/ { next } { count[$1]++ } END { for (s in count) printf "%s %d\n", s, count[s] }' \
      "$manifest" | sort
    ;;

  verify)
    lines=$(entries "$@")
    missing=0
    bad=0
    while read -r set sha path url; do
      file="$dest/$path"
      if [[ ! -f $file ]]; then
        missing=$((missing + 1))
        continue
      fi
      if [[ $(sha256_of "$file") != "$sha" ]]; then
        printf 'MISMATCH %s (%s)\n' "$path" "$set" >&2
        bad=$((bad + 1))
      fi
    done <<< "$lines"
    if (( bad )); then
      printf 'verify: %d file(s) differ from their pins; run fetch to replace them\n' "$bad" >&2
      exit 1
    fi
    if (( missing )); then
      printf 'verify: %d file(s) missing; run fetch\n' "$missing" >&2
      exit 3
    fi
    printf 'verify: all present files match their pins\n'
    ;;

  fetch)
    lines=$(entries "$@")
    fetched=0
    while read -r set sha path url; do
      file="$dest/$path"
      if [[ -f $file && $(sha256_of "$file") == "$sha" ]]; then
        continue
      fi
      mkdir -p "$(dirname "$file")"
      part="$file.part"
      if ! curl -sfL --retry 3 -o "$part" "$url"; then
        rm -f "$part"
        printf 'fetch: download failed: %s\n' "$url" >&2
        exit 4
      fi
      got=$(sha256_of "$part")
      if [[ $got != "$sha" ]]; then
        rm -f "$part"
        printf 'fetch: %s does not match its pin (%s): expected %s, got %s\n' \
          "$path" "$set" "$sha" "$got" >&2
        exit 1
      fi
      mv -f "$part" "$file"
      fetched=$((fetched + 1))
    done <<< "$lines"
    printf 'fetch: %d file(s) downloaded; everything selected matches its pin\n' "$fetched"
    ;;

  *) usage ;;
esac
