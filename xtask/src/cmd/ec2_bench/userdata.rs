//! Renders the cloud-init user-data script the instance self-drives with.
//!
//! Security: every user-controlled value travels inside a base64-encoded JSON
//! params blob that the script decodes at runtime and reads via `jq` into
//! quoted shell variables. No user value is ever interpolated into the script
//! text, so a hostile ref/bucket/tag cannot inject shell commands.

use crate::cmd::ec2_bench::validate::RunMode;

/// Parameters substituted into the (otherwise static) user-data script.
#[derive(Debug, serde::Serialize)]
pub struct UserDataParams {
    /// Remote to clone (must be publicly clonable).
    pub repo_url: String,
    /// Base ref resolved to a SHA.
    pub base_sha: String,
    /// Head ref resolved to a SHA.
    pub head_sha: String,
    /// Ref the rendering `xtask` is built from (the invoking CLI's own ref).
    pub tooling_sha: String,
    /// Rust toolchain (`nightly` or `nightly-YYYY-MM-DD`).
    pub toolchain: String,
    /// `compare` (two-ref A/B/B/A) or `charts` (single-ref).
    pub mode: RunMode,
    /// Reduce sample count via `BENCH_QUICK` and skip the interleave replicate.
    pub quick: bool,
    /// Destination bucket.
    pub s3_bucket: String,
    /// Destination key prefix (ends with `/`).
    pub s3_prefix: String,
    /// Instance-side hard-deadline minutes.
    pub max_runtime_min: u32,
}

/// Standard (RFC 4648) base64 with `=` padding.
#[must_use]
pub fn base64_encode(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        out.push(char::from(A[usize::from(b0 >> 2)]));
        out.push(char::from(A[usize::from(((b0 & 0x03) << 4) | (b1 >> 4))]));
        out.push(if chunk.len() > 1 {
            char::from(A[usize::from(((b1 & 0x0f) << 2) | (b2 >> 6))])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(A[usize::from(b2 & 0x3f)])
        } else {
            '='
        });
    }
    out
}

/// Render the full user-data script. The only substitution is the base64 params
/// blob (base64 contains none of `{`, `}`, or `'`), so `SCRIPT` stays literal.
#[must_use]
pub fn render_user_data(params: &UserDataParams) -> String {
    // serde_json cannot fail on this fixed, string-only struct.
    let json = serde_json::to_vec(params).unwrap_or_default();
    let b64 = base64_encode(&json);
    SCRIPT.replace("__PARAMS_B64__", &b64)
}

/// The static self-driving script. Reads all inputs from the decoded params
/// file via `jq`; the EXIT trap always uploads a log + `status.json`, cancels
/// the boot deadline, and powers off (the instance is launched with
/// shutdown-behavior=terminate).
const SCRIPT: &str = r#"#!/bin/bash
set -euo pipefail

LOG=/var/log/bench.log
exec > >(tee -a "$LOG") 2>&1

# Hardcoded backstop deadline, armed before anything that can hang or fail (apt,
# network) and before the EXIT trap exists. The precise per-run deadline replaces
# it once params parse; on_exit cancels it on a clean finish. 720 min is well
# above the max runtime, so it never fires on a healthy run.
shutdown -h +720 || true

printf '%s' '__PARAMS_B64__' | base64 -d > /run/bench-params.json

# jq is the only bootstrap dependency; everything else is read as data.
export DEBIAN_FRONTEND=noninteractive
apt-get update -y
apt-get install -y jq git build-essential ca-certificates curl

P() { jq -r "$1" /run/bench-params.json; }
REPO_URL="$(P .repo_url)"
BASE_SHA="$(P .base_sha)"
HEAD_SHA="$(P .head_sha)"
TOOLING_SHA="$(P .tooling_sha)"
TOOLCHAIN="$(P .toolchain)"
MODE="$(P .mode)"
QUICK="$(P .quick)"
S3_BUCKET="$(P .s3_bucket)"
S3_PREFIX="$(P .s3_prefix)"
MAX_RUNTIME_MIN="$(P .max_runtime_min)"

s3put() { aws s3 cp "$1" "s3://${S3_BUCKET}/${S3_PREFIX}$2" --only-show-errors; }

STATUS=failed
on_exit() {
  local code=$?
  [ "$code" -eq 0 ] && STATUS=done
  jq -n --arg status "$STATUS" --arg base "$BASE_SHA" --arg head "$HEAD_SHA" \
        --argjson code "$code" \
        '{schemaVersion:1, status:$status, exitCode:$code, base:$base, head:$head}' \
        > /run/status.json || true
  s3put "$LOG" bench.log || true
  s3put /run/status.json status.json || true
  shutdown -c || true
  shutdown -h now || true
}
trap on_exit EXIT

# Instance-side hard deadline: fires even if this script hangs or the operator's
# machine sleeps. Cancelled by on_exit on a clean finish.
shutdown -h +"$MAX_RUNTIME_MIN"

# --- CPU tuning (best-effort; a missing knob must not abort the run) ----------
echo off > /sys/devices/system/cpu/smt/control 2>/dev/null || true
echo 1 > /sys/devices/system/cpu/intel_pstate/no_turbo 2>/dev/null || true
for g in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
  echo performance > "$g" 2>/dev/null || true
done

# --- Toolchain ----------------------------------------------------------------
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y --default-toolchain "$TOOLCHAIN" --profile minimal
# shellcheck disable=SC1091
. "$HOME/.cargo/env"
cargo install --locked cargo-criterion

# --- AWS CLI (for uploads; instance role provides credentials) ----------------
apt-get install -y awscli

# --- Clone + fetch the exact SHAs (-- guards against option injection) --------
git clone -- "$REPO_URL" repo
cd repo
git fetch --quiet origin "$BASE_SHA" "$HEAD_SHA" "$TOOLING_SHA"

# Build the rendering xtask from the tooling ref, decoupled from base/head.
git checkout --quiet "$TOOLING_SHA"
cargo build --release -p xtask
XTASK="$(pwd)/target/release/xtask"

export BENCH_QUICK
if [ "$QUICK" = "true" ]; then BENCH_QUICK=1; else BENCH_QUICK=; fi
capture() { git checkout --quiet "$1"; cargo criterion -p arity-arrays --message-format=json > "$2"; }

if [ "$MODE" = "compare" ]; then
  capture "$BASE_SHA" base1.json
  capture "$HEAD_SHA" head1.json
  if [ "$QUICK" != "true" ]; then
    capture "$HEAD_SHA" head2.json
    capture "$BASE_SHA" base2.json
    "$XTASK" compare --head head1.json head2.json --base base1.json base2.json > compare.md
    for f in base1.json base2.json head1.json head2.json compare.md; do s3put "$f" "$f"; done
  else
    "$XTASK" compare --head head1.json --base base1.json > compare.md
    for f in base1.json head1.json compare.md; do s3put "$f" "$f"; done
  fi
else
  capture "$HEAD_SHA" run.json
  "$XTASK" charts run.json
  s3put run.json run.json
  for svg in docs/bench/*.svg; do [ -e "$svg" ] && s3put "$svg" "$(basename "$svg")"; done
fi
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> UserDataParams {
        UserDataParams {
            repo_url: "https://github.com/demosdemon/arity-arrays.git".into(),
            base_sha: "aaaaaaa".into(),
            head_sha: "bbbbbbb".into(),
            tooling_sha: "ccccccc".into(),
            toolchain: "nightly".into(),
            mode: RunMode::Compare,
            quick: false,
            s3_bucket: "my-bucket".into(),
            s3_prefix: "arity-bench/run/".into(),
            max_runtime_min: 360,
        }
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn script_has_shebang_and_strict_mode() {
        let s = render_user_data(&sample());
        assert!(s.starts_with("#!/bin/bash\n"));
        assert!(s.contains("set -euo pipefail"));
    }

    #[test]
    fn script_arms_deadline_and_cancels_on_exit() {
        let s = render_user_data(&sample());
        assert!(s.contains("shutdown -h +\"$MAX_RUNTIME_MIN\""));
        assert!(s.contains("shutdown -c"));
        assert!(s.contains("trap on_exit EXIT"));
    }

    #[test]
    fn script_arms_backstop_deadline_before_bootstrap() {
        let s = render_user_data(&sample());
        let backstop = s.find("shutdown -h +720").expect("backstop deadline");
        let apt = s.find("apt-get update").expect("apt bootstrap");
        assert!(backstop < apt, "backstop must be armed before apt runs");
    }

    #[test]
    fn script_uses_verbatim_criterion_invocation() {
        let s = render_user_data(&sample());
        assert!(s.contains("cargo criterion -p arity-arrays --message-format=json"));
    }

    #[test]
    fn script_clones_with_argument_guard() {
        let s = render_user_data(&sample());
        // `--` stops option injection even though the URL is validated.
        assert!(s.contains("git clone -- \"$REPO_URL\""));
    }

    #[test]
    fn user_values_appear_only_inside_the_base64_blob() {
        let mut p = sample();
        p.s3_bucket = "evil$(rm -rf /)".into();
        let s = render_user_data(&p);
        // The dangerous literal must NOT appear as raw script text.
        assert!(!s.contains("rm -rf /"));
        // It IS present, encoded, in the params blob (decode to confirm).
        let b64_line = s
            .lines()
            .find(|l| l.contains("base64 -d"))
            .expect("params line");
        // The blob is the single-quoted token before ` | base64 -d`. The line
        // also has an unrelated `'%s'` quoted token earlier (the `printf`
        // format string), so find the *last* quoted token by taking the two
        // right-most quotes rather than the first-to-last span.
        let end = b64_line.rfind('\'').unwrap();
        let start = b64_line[..end].rfind('\'').unwrap() + 1;
        let decoded = decode_for_test(&b64_line[start..end]);
        assert!(String::from_utf8_lossy(&decoded).contains("evil$(rm -rf /)"));
    }

    // Minimal base64 decoder used only to assert the round-trip.
    fn decode_for_test(s: &str) -> Vec<u8> {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut lut = [255u8; 256];
        for (i, &c) in A.iter().enumerate() {
            // `A` has exactly 64 entries, so `i` is always < 64: well under `u8::MAX`.
            lut[usize::from(c)] = u8::try_from(i).expect("alphabet index fits in u8");
        }
        let mut out = Vec::new();
        let mut buf = 0u32;
        let mut bits = 0;
        for &c in s.as_bytes() {
            if c == b'=' {
                break;
            }
            let v = lut[usize::from(c)];
            assert_ne!(v, 255, "bad base64");
            buf = (buf << 6) | u32::from(v);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                // Masking to the low 8 bits mirrors a truncating `as u8` cast,
                // but proves losslessness to the compiler via `try_from`.
                out.push(u8::try_from((buf >> bits) & 0xff).expect("masked to 8 bits"));
            }
        }
        out
    }
}
