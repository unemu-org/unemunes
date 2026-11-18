#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Keep tracked and untracked files safe to publish. Adapted from unemupc's
# scripts/publication-guard.sh (unemupc), with a check of its own for game
# images: no commercial ROM ever enters this repository.
#
# Run scripts/publication-guard.sh from any directory. It scans tracked files
# and untracked, non-ignored files. Every check reads with LC_ALL=C and needs
# only bash 3.2, grep, sed, awk, head and od, so it runs the same on macOS and
# Linux without the devShell.
#
# The text checks skip the guard itself, because it defines the forbidden
# spellings, and Cargo.lock, which is generated dependency metadata. Cargo.lock
# has its own check: every dependency comes from crates.io or a github.com git
# repository.
#
# Checks:
# - private filesystem paths: home directories, macOS volumes and temporary
#   directories, Windows profiles, /root, and /mnt, /media and /run/media mounts;
# - e-mail addresses outside example domains and the allowlist below; the user
#   of a URL or of an scp-style remote (an SSH login) is not an e-mail address;
# - RFC 1918 and carrier-grade NAT (100.64.0.0/10) IPv4 addresses; network
#   ranges in CIDR notation are not addresses;
# - private names, read only from the comma-separated environment variable
#   PUBLICATION_GUARD_PRIVATE_HOSTS so they never appear in the repository;
#   without it, this check is skipped, so it runs only where someone sets it,
#   never in CI. A hit prints only the file and line, never the name;
# - game images: files named like NES, Famicom Disk System or NSF images, and
#   files of any name whose first bytes are the signature of one, outside the
#   allowlist of open test images below.
set -euo pipefail
export LC_ALL=C

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Absolute private path forms. The ones without a user directory must start a
# path, so a URL or a relative path that merely contains them does not match.
path_start='(^|[^[:alnum:]_.~-])'
pattern='/home/[^/[:space:]]+|/Users/[^/[:space:]]+|~/src/'
pattern+="|${path_start}(/volumes/|/private/(var|tmp)/|/var/folders/|/root/|/mnt/[^/[:space:]]+"
pattern+='|/media/[^/[:space:]]+/|/run/media/[^/[:space:]]+/)'
pattern+='|[[:alpha:]]:[/\\]+users[/\\]'
# An address preceded by '/' is a URL's user information, not an e-mail address.
# The optional tail catches the ':path' of an scp-style remote, which the
# report then skips.
email_re='(^|[^/[:alnum:]._%+-])[[:alnum:]._%+-]+@[[:alnum:]-]+(\.[[:alnum:]-]+)*\.[[:alpha:]]{2,}(:[[:alnum:]_.~/-])?'
email_remote=':[^:]$'
# The commit trailer of the coding agent and example domains.
email_allowed='^(noreply@anthropic\.com|.*@(([[:alnum:]-]+\.)*example\.(org|com|net)|([[:alnum:]-]+\.)+example))$'
# RFC 1918 and carrier-grade NAT; an optional prefix length marks a CIDR range,
# which the report skips.
private_ip_re='(^|[^[:alnum:].])(10(\.[0-9]{1,3}){3}|172\.(1[6-9]|2[0-9]|3[01])(\.[0-9]{1,3}){2}|192\.168(\.[0-9]{1,3}){2}'
private_ip_re+='|100\.(6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])(\.[0-9]{1,3}){2})(/[0-9]{1,2})?($|[^0-9.]|\.$|\.[^0-9])'
private_ip_range='[0-9]/[0-9]'
private_ip_allowed='^$'
lock_source_allowed='^source = "(registry\+https://github\.com/rust-lang/crates\.io-index|sparse\+https://index\.crates\.io/|git\+https://github\.com/[^"[:space:]]+)"$'
rc=0

# Open test images that may be committed, each beside its licence text. A path
# is added here only by the change that admits the image, after its licence was
# checked. Commercial images are never listed: hashes and titles are fine,
# bytes are not.
rom_allowed() {
  case "$1" in
    *) return 1 ;;
  esac
}

# What kind of game image a file is, from its name or its first bytes; empty
# when it is none.
rom_kind() {
  local magic
  case "$1" in
    *.[nN][eE][sS]) printf '%s' 'NES image (by name)'; return ;;
    *.[fF][dD][sS]|*.[qQ][dD]) printf '%s' 'Famicom Disk System image (by name)'; return ;;
    *.[uU][nN][fF]|*.[uU][nN][iI][fF]) printf '%s' 'UNIF image (by name)'; return ;;
    *.[nN][sS][fF]|*.[nN][sS][fF][eE]) printf '%s' 'NSF music image (by name)'; return ;;
  esac
  magic=$(head -c 15 -- "$1" | od -An -tx1 -v | tr -d ' \n')
  case "$magic" in
    4e45531a*) printf '%s' 'iNES or NES 2.0 image (by signature)' ;;
    4644531a*) printf '%s' 'Famicom Disk System image with header (by signature)' ;;
    012a4e494e54454e444f2d4856432a*) printf '%s' 'Famicom Disk System side (by signature)' ;;
    554e4946*) printf '%s' 'UNIF image (by signature)' ;;
    4e45534d1a*|4e534645*) printf '%s' 'NSF music image (by signature)' ;;
  esac
}

all_files=()
files=()
while IFS= read -r -d '' file; do
  [[ -f $file ]] || continue
  all_files+=("$file")
  case "$file" in
    scripts/publication-guard.sh|Cargo.lock) ;;
    *) files+=("$file") ;;
  esac
done < <(git ls-files --cached --others --exclude-standard -z)

# Print the hits of one check whose matches are filtered by an allowlist:
# grep's "file:line:match" lines on stdin; matches containing the ERE in $3
# are not values at all; the rest are reduced to the bare value by the pattern
# in $1, then values matching the ERE in $2 are dropped.
# The patterns travel in the environment: awk -v would rewrite their backslashes.
report_unlisted() {
  GUARD_VALUE="$1" GUARD_ALLOWED="$2" GUARD_SKIP="$3" awk '{
    text = $0
    sub(/^[^:]*:[0-9]*:/, "", text)
    if (text ~ ENVIRON["GUARD_SKIP"]) next
    if (!match(text, ENVIRON["GUARD_VALUE"])) next
    found = tolower(substr(text, RSTART, RLENGTH))
    if (found ~ ENVIRON["GUARD_ALLOWED"]) next
    print
  }'
}

# Run grep over every scanned file at once; status 1 means no match.
scan() {
  local status=0
  LC_ALL=C grep "$@" || status=$?
  if (( status > 1 )); then
    exit "$status"
  fi
}

for file in ${all_files[@]+"${all_files[@]}"}; do
  kind=$(rom_kind "$file")
  if [[ -n $kind ]] && ! rom_allowed "$file"; then
    printf '%s\n' "publication guard: $kind outside the allowlist: $file" >&2
    rc=1
  fi
done

if (( ${#files[@]} )); then
  hits=$(scan -HIniE -- "$pattern" "${files[@]}")
  if [[ -n $hits ]]; then
    printf '%s\n' "$hits"
    printf '%s\n' 'publication guard: private filesystem path' >&2
    rc=1
  fi

  hits=$(scan -HIinoE -- "$email_re" "${files[@]}" |
    report_unlisted '[[:alnum:]._%+-]+@[[:alnum:].-]*[[:alnum:]]' "$email_allowed" "$email_remote")
  if [[ -n $hits ]]; then
    printf '%s\n' "$hits"
    printf '%s\n' 'publication guard: e-mail address outside the allowlist' >&2
    rc=1
  fi

  hits=$(scan -HIinoE -- "$private_ip_re" "${files[@]}" |
    report_unlisted '[0-9]+[.][0-9]+[.][0-9]+[.][0-9]+' "$private_ip_allowed" "$private_ip_range")
  if [[ -n $hits ]]; then
    printf '%s\n' "$hits"
    printf '%s\n' 'publication guard: private IPv4 address' >&2
    rc=1
  fi
fi

host_patterns=()
IFS=',' read -r -a hosts <<< "${PUBLICATION_GUARD_PRIVATE_HOSTS:-}"
for host in ${hosts[@]+"${hosts[@]}"}; do
  host=$(printf '%s' "$host" | LC_ALL=C sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//')
  if [[ -n $host ]]; then
    host_patterns+=(-e "$host")
  fi
done
if (( ${#host_patterns[@]} && ${#all_files[@]} )); then
  # Only file and line: the names stay out of logs and terminals.
  hits=$(scan -HIinF "${host_patterns[@]}" -- "${all_files[@]}" | cut -d: -f1,2)
  if [[ -n $hits ]]; then
    printf '%s\n' "$hits"
    printf '%s\n' 'publication guard: private name from PUBLICATION_GUARD_PRIVATE_HOSTS' >&2
    rc=1
  fi
fi

if [[ -f Cargo.lock ]]; then
  hits=$(scan -n '^source = ' Cargo.lock |
    LC_ALL=C GUARD_ALLOWED="$lock_source_allowed" awk '{
      text = $0
      sub(/^[0-9]*:/, "", text)
      if (text !~ ENVIRON["GUARD_ALLOWED"]) print "Cargo.lock:" $0
    }')
  if [[ -n $hits ]]; then
    printf '%s\n' "$hits"
    printf '%s\n' 'publication guard: Cargo.lock source outside crates.io and github.com' >&2
    rc=1
  fi
fi

if (( rc )); then
  printf '%s\n' 'publication guard: fix the publication diagnostics above.' >&2
fi
exit "$rc"
