#!/usr/bin/env bash
#
# Interleaved multi-profile benchmark: what does `lto-probe` actually buy, and
# which half of the profile buys it?
#
# THE PROBLEM THIS EXISTS TO SOLVE
#
# `[profile.lto-probe]` moves TWO knobs at once relative to the default `bench`
# profile: `lto = "fat"` AND `codegen-units = 1`. Benchmarking it against the
# default therefore cannot attribute a delta to link-time optimization --
# `codegen-units = 1` is a well-known speedup entirely on its own. A third arm
# that moves only codegen-units separates them:
#
#   A   lto=false  codegen-units=16   the default `bench` profile
#   B   lto=fat    codegen-units=1    == [profile.lto-probe]
#   C   lto=false  codegen-units=1    isolates codegen-units
#
#   C vs A   the codegen-units effect
#   B vs C   LTO's marginal effect, holding codegen-units fixed  <-- the claim
#            in crates/arity-arrays/README.md rests on this contrast, not on
#            B vs A
#   B vs A   the combined effect a user enabling lto-probe would see
#
# HOW THE ARMS ARE SELECTED
#
# Via `CARGO_PROFILE_BENCH_*` environment overrides, not `--profile`.
# cargo-criterion rejects `--profile`, but it builds with the `bench` profile
# (it reports `Finished \`bench\` profile`), and those variables override that
# profile's keys. This keeps the `--message-format=json` capture that `xtask`
# ingests -- plain `cargo bench --profile X` would switch profiles but emit no
# such JSON.
#
# Arm A pins cargo's current release/bench defaults explicitly rather than
# leaving them unset, so all three arms are specified the same way and the
# experiment does not silently redefine itself if a cargo default changes.
#
# WHY THE PALINDROME ORDER
#
# Arms run as A B C C B A. Each arm's captures then straddle the run's midpoint
# symmetrically -- A at slots 1+6, B at 2+5, C at 3+4, every centroid 3.5 -- so
# slow *linear* thermal/turbo drift cancels within each pairwise differential
# instead of merely being shared by it. This is the same reasoning as the
# base/head/head/base interleave in .github/workflows/bench-compare.yml,
# generalized: a palindrome gives every arm an identical centroid at any arm
# count. `xtask compare` averages each arm's captures (point = mean, interval =
# envelope).
#
# Every arm is built before any arm is timed. Cargo fingerprints per profile
# configuration and keeps all three binaries side by side, so no capture pays a
# rebuild and the slots stay symmetric in a way the CI workflow's
# back-to-back-head pair does not.
#
# THE NOISE FLOOR IS FREE
#
# Arm A brackets the run (slots 1 and 6), so comparing its own two captures to
# each other measures drift plus measurement noise directly -- same profile,
# same binary, maximally separated in time. The report leads with that as the
# significance bar for the three contrasts: a row that moves less than its
# noise-floor counterpart has not been shown to move. Arm A is used rather than
# B or C because its slots are the furthest apart, which makes it the worst
# case and therefore the conservative bar. Full-precision runs only -- quick
# mode has one capture per arm and nothing to compare.
#
# NOTES
#
# * No `--all-features`: no bench code is feature-gated, so it would add
#   compile time and no coverage. Matches `just bench-export` and the CI
#   capture step.
# * Quick mode sets `BENCH_QUICK=1` rather than passing `--quick`:
#   cargo-criterion forwards no criterion CLI flag to the harness. See
#   crates/arity-arrays/benches/quick_criterion.rs.
# * `--update-docs` regenerates the README tables and docs/bench SVGs from arm
#   **A** only. The published numbers describe what a downstream consumer gets,
#   and a consumer picks its own profile -- the default one is the honest
#   subject. B and C exist to answer the LTO question, not to be published.
#
# Usage:
#   scripts/bench-ab.sh [--quick] [--label NAME] [--update-docs]
#
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

QUICK=0
LABEL=ab
UPDATE_DOCS=0

usage() {
  cat << 'EOF'
Interleaved multi-profile benchmark: default vs lto-probe vs codegen-units-only.

usage: scripts/bench-ab.sh [--quick] [--label NAME] [--update-docs]

  --quick         BENCH_QUICK=1 and a single pass (A B C). Directional only.
  --label NAME    capture prefix under bench-data/ (default: ab).
  --update-docs   republish README tables + docs/bench SVGs from arm A.
                  Refused on --quick runs.

Arms:
  A  lto=false codegen-units=16   default bench profile
  B  lto=fat   codegen-units=1    == [profile.lto-probe]
  C  lto=false codegen-units=1    isolates codegen-units

See the comment header of this file for why three arms and why a palindrome.
EOF
  exit "${1:-0}"
}

while [ $# -gt 0 ]; do
  case "$1" in
  --quick)
    QUICK=1
    shift
    ;;
  --update-docs)
    UPDATE_DOCS=1
    shift
    ;;
  --label)
    LABEL=${2:?--label needs a value}
    shift 2
    ;;
  -h | --help) usage 0 ;;
  *)
    echo "unknown argument: $1" >&2
    usage 2
    ;;
  esac
done

# The arm table. To add or change an arm, edit here and in ORDER below; keep
# ORDER a palindrome so every arm keeps the same centroid.
arm_config() {
  case "$1" in
  A) echo "false 16" ;;
  B) echo "fat 1" ;;
  C) echo "false 1" ;;
  *)
    echo "unknown arm: $1" >&2
    return 1
    ;;
  esac
}

arm_desc() {
  case "$1" in
  A) echo "default bench profile (cargo's release defaults)" ;;
  B) echo "lto-probe: lto=fat + codegen-units=1" ;;
  C) echo "codegen-units=1 only (isolates the confound)" ;;
  esac
}

OUTDIR=bench-data
mkdir -p "$OUTDIR"

if [ "$QUICK" -eq 1 ]; then
  # Single pass: replicates buy drift cancellation, which is meaningless at
  # quick mode's noise level. Mirrors the CI workflow skipping its second
  # pair on quick runs.
  ORDER=(A B C)
  export BENCH_QUICK=1
else
  ORDER=(A B C C B A)
fi

# Build xtask once, up front: it fails fast if the tool is broken, before the
# long captures, and keeps `cargo run` out of the timed section.
echo "==> building xtask"
cargo build --release -q -p xtask
XTASK=target/release/xtask

echo "==> pre-building every arm (so no timed slot pays a rebuild)"
for arm in A B C; do
  read -r lto cgu <<< "$(arm_config "$arm")"
  echo "    arm $arm: lto=$lto codegen-units=$cgu -- $(arm_desc "$arm")"
  CARGO_PROFILE_BENCH_LTO="$lto" \
    CARGO_PROFILE_BENCH_CODEGEN_UNITS="$cgu" \
    cargo criterion --no-run -p arity-arrays
done

# Clear this arm's previous captures so a shortened re-run cannot leave stale
# files behind for the globs below to pick up.
rm -f "$OUTDIR/${LABEL}"-[0-9][0-9]-[ABC].json

echo "==> running ${#ORDER[@]} captures: ${ORDER[*]}"
slot=1
for arm in "${ORDER[@]}"; do
  read -r lto cgu <<< "$(arm_config "$arm")"
  out=$(printf '%s/%s-%02d-%s.json' "$OUTDIR" "$LABEL" "$slot" "$arm")
  echo "==> slot $slot/${#ORDER[@]}: arm $arm (lto=$lto codegen-units=$cgu) -> $out"
  CARGO_PROFILE_BENCH_LTO="$lto" \
    CARGO_PROFILE_BENCH_CODEGEN_UNITS="$cgu" \
    cargo criterion -p arity-arrays --message-format=json > "$out"
  slot=$((slot + 1))
done

# `??` matches the two-digit slot number rather than `[0-9][0-9]`: shfmt parses a
# bracket glob at the head of an array element as an array subscript and fails.
# The slot is always `%02d`-formatted above, so the two are equivalent here.
shopt -s nullglob
A_FILES=("$OUTDIR/${LABEL}"-??-A.json)
B_FILES=("$OUTDIR/${LABEL}"-??-B.json)
C_FILES=("$OUTDIR/${LABEL}"-??-C.json)
shopt -u nullglob

if [ ${#A_FILES[@]} -eq 0 ] || [ ${#B_FILES[@]} -eq 0 ] || [ ${#C_FILES[@]} -eq 0 ]; then
  echo "missing captures (A=${#A_FILES[@]} B=${#B_FILES[@]} C=${#C_FILES[@]}) -- nothing to compare" >&2
  exit 1
fi

REPORT="$OUTDIR/${LABEL}-report.md"
{
  note=''
  [ "$QUICK" -eq 1 ] && note=' (quick: single pass, no interleave)'
  cat << EOF
# Profile A/B report: \`$LABEL\`

Order: \`${ORDER[*]}\`$note

EOF
  for arm in A B C; do
    read -r lto cgu <<< "$(arm_config "$arm")"
    cat << EOF
- **$arm** — \`lto=$lto\`, \`codegen-units=$cgu\` — $(arm_desc "$arm")
EOF
  done
  printf '\n'
  if [ "$QUICK" -eq 1 ]; then
    cat << 'EOF'
> [!NOTE]
> Quick mode: reduced sample count, single-pass (no interleaving).
> Directional only — do not publish these numbers.

EOF
  fi

  # The yardstick comes first: without it there is no way to tell which of
  # the deltas below are effects and which are weather.
  if [ ${#A_FILES[@]} -ge 2 ]; then
    first_a=${A_FILES[0]}
    last_a=${A_FILES[${#A_FILES[@]} - 1]}
    cat << EOF
## Apparatus noise floor (arm A, first vs last capture)

\`$(basename "$first_a")\` vs \`$(basename "$last_a")\` — the **same profile**
measured at the run's two extremes. Nothing here is a profile effect: every
delta is drift plus measurement noise, and because these two slots are the
furthest apart in wall-clock time, this is the *worst case* of it.

Read it as the significance bar for the three contrasts below. A row that moves
less than its noise-floor counterpart has not been shown to move at all. The
palindrome is what earns that reading: arm A brackets the run, so its own
spread bounds the drift that the interleave then cancels out of B and C.

EOF
    "$XTASK" compare --base "$first_a" --head "$last_a"
    printf '\n'
  fi

  cat << 'EOF'
## codegen-units effect (C vs A)

Moves `codegen-units` 16 -> 1 with `lto` held at `false`.

EOF
  "$XTASK" compare --base "${A_FILES[@]}" --head "${C_FILES[@]}"

  cat << 'EOF'

## LTO marginal effect (B vs C)

Moves `lto` `false` -> `fat` with `codegen-units` held at 1. This is the
contrast the README's LTO claim rests on — not the combined one below.

EOF
  "$XTASK" compare --base "${C_FILES[@]}" --head "${B_FILES[@]}"

  cat << 'EOF'

## Combined lto-probe effect (B vs A)

Both knobs at once — what a user enabling `lto-probe` would see. Not
attributable to LTO on its own; see the two contrasts above.

EOF
  "$XTASK" compare --base "${A_FILES[@]}" --head "${B_FILES[@]}"

  cat << 'EOF'

## Reading this

Two checks before believing any row above.

**Is it bigger than the weather?** Compare it against the same row in the noise
floor. A delta at or under that magnitude is not evidence of anything.

**Is it this crate?** If a delta appears uniformly across *every* subject —
including `HashMap` and `BTreeMap`, whose code this crate's `#[inline]`
annotations cannot reach — then the profile is optimizing criterion's own timing
loop, not this crate. A real effect concentrates in the arity types.
EOF
} > "$REPORT"

echo
echo "==> report: $REPORT"
echo

if [ "$UPDATE_DOCS" -eq 1 ]; then
  if [ "$QUICK" -eq 1 ]; then
    echo "refusing --update-docs on a --quick run: those numbers are directional only" >&2
    exit 1
  fi
  echo "==> regenerating README tables + docs/bench SVGs from arm A (${#A_FILES[@]} captures)"
  "$XTASK" charts --head "${A_FILES[@]}"
fi

cat "$REPORT"
