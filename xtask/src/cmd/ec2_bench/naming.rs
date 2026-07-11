//! Deterministic run identifiers and derived resource names.
//!
//! The run-id format `<UTC-compact-timestamp>-<6hex>` and the
//! `arity-ec2-bench-<run-id>` resource-name convention are a stable interface:
//! `teardown --run-id` derives IAM role/profile names from them, and changing
//! either orphans pre-change resources. Keep the total IAM name length under
//! AWS's 64-character limit.

use std::fmt;
use std::path::PathBuf;

/// A unique run identifier: `<UTC-compact-timestamp>-<6hex>`,
/// e.g. `20260709t143000z-a1b2c3`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunId(String);

impl RunId {
    /// Build a run-id from a Unix timestamp (seconds) and a 24-bit salt.
    ///
    /// Splitting the inputs keeps this pure and unit-testable; callers pass
    /// `SystemTime::now()` seconds and low-entropy salt (e.g. sub-second
    /// nanos).
    #[must_use]
    pub fn generate(epoch_secs: u64, salt: u32) -> Self {
        let (y, mo, d, h, mi, s) = civil_from_epoch(epoch_secs);
        Self(format!(
            "{y:04}{mo:02}{d:02}t{h:02}{mi:02}{s:02}z-{:06x}",
            salt & 0x00ff_ffff
        ))
    }

    /// The run-id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Shared name for the auto-created IAM role, instance profile, and security
/// group: `arity-ec2-bench-<run-id>`.
#[must_use]
pub fn resource_name(run: &RunId) -> String {
    format!("arity-ec2-bench-{run}")
}

/// Default S3 key prefix for a run: `arity-bench/<run-id>/`.
#[must_use]
pub fn default_s3_prefix(run: &RunId) -> String {
    format!("arity-bench/{run}/")
}

/// Join an S3 prefix and a file name with exactly one separating slash.
#[must_use]
pub fn s3_key(prefix: &str, file: &str) -> String {
    format!("{}/{}", prefix.trim_end_matches('/'), file)
}

/// Default local directory for downloaded artifacts: `bench-data/ec2-<run-id>`.
#[must_use]
pub fn default_output_dir(run: &RunId) -> PathBuf {
    PathBuf::from(format!("bench-data/ec2-{run}"))
}

/// Convert Unix-epoch seconds to civil `(year, month, day, hour, min, sec)` in
/// UTC via Howard Hinnant's `civil_from_days` algorithm, with no external date
/// dependency. Inputs come from `SystemTime::now()`, well within the
/// algorithm's documented range for present-day timestamps.
const fn civil_from_epoch(epoch_secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    // `epoch_secs` is unsigned, so `days` is always non-negative. The
    // largest possible `u64` divided by 86_400 is ~2.1e14, far under
    // `i64::MAX` (~9.2e18), so this u64 -> i64 conversion never wraps.
    #[expect(
        clippy::cast_possible_wrap,
        reason = "epoch_secs / 86_400 <= u64::MAX / 86_400 (~2.1e14), far under i64::MAX"
    )]
    let days = (epoch_secs / 86_400) as i64;
    let rem = epoch_secs % 86_400;
    // `rem` is bounded to [0, 86_399] by the modulo above, so `hour < 24`,
    // `mi < 60`, and `sec < 60`: each narrows into u32 losslessly.
    let (hour, mi, sec) = (
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    );

    // Shift epoch to an era-based day count anchored at 0000-03-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    // `doy` and `mp` are bounded to [0, 365] and [0, 11] by the
    // civil_from_days invariants above, so `d` is always in [1, 31].
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "doy in [0, 365] and mp in [0, 11], so d is in [1, 31]: always fits u32"
    )]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    // `mp` is bounded to [0, 11], so `m` is always in [1, 12].
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "mp is in [0, 11], so m is in [1, 12]: always fits u32"
    )]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, hour, mi, sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_formats_timestamp_and_salt() {
        // 2026-07-09T14:30:00Z == 1783607400 seconds since the Unix epoch.
        let id = RunId::generate(1_783_607_400, 0x00a1_b2c3);
        assert_eq!(id.as_str(), "20260709t143000z-a1b2c3");
    }

    #[test]
    fn run_id_masks_salt_to_24_bits() {
        // Only the low 24 bits become the 6 hex digits.
        let id = RunId::generate(0, 0xffff_ffff);
        assert!(id.as_str().ends_with("-ffffff"), "{}", id.as_str());
    }

    #[test]
    fn epoch_zero_is_unix_epoch() {
        let id = RunId::generate(0, 0);
        assert_eq!(id.as_str(), "19700101t000000z-000000");
    }

    #[test]
    fn handles_leap_day() {
        // 2024-02-29T00:00:00Z == 1709164800.
        let id = RunId::generate(1_709_164_800, 0);
        assert!(
            id.as_str().starts_with("20240229t000000z"),
            "{}",
            id.as_str()
        );
    }

    #[test]
    fn resource_name_stays_under_iam_limit() {
        let id = RunId::generate(1_783_607_400, 0x00a1_b2c3);
        let name = resource_name(&id);
        assert_eq!(name, "arity-ec2-bench-20260709t143000z-a1b2c3");
        assert!(name.len() <= 64, "IAM name too long: {}", name.len());
    }

    #[test]
    fn s3_prefix_and_key_join_cleanly() {
        let id = RunId::generate(0, 0);
        assert_eq!(
            default_s3_prefix(&id),
            "arity-bench/19700101t000000z-000000/"
        );
        assert_eq!(
            s3_key("arity-bench/run/", "compare.md"),
            "arity-bench/run/compare.md"
        );
        assert_eq!(s3_key("no-slash", "f.json"), "no-slash/f.json");
    }

    #[test]
    fn output_dir_is_under_bench_data() {
        let id = RunId::generate(0, 0);
        assert_eq!(
            default_output_dir(&id),
            PathBuf::from("bench-data/ec2-19700101t000000z-000000")
        );
    }
}
