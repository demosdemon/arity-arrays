//! Poll S3 for the instance's `status.json` marker, detect spot interruption
//! (instance gone with no marker), and download artifacts on completion.
#![expect(
    clippy::future_not_send,
    reason = "AwsBackend futures are driven on a current-thread runtime; Send is unnecessary"
)]

use std::path::Path;
use std::time::Duration;

use crate::cmd::ec2_bench::aws::AwsBackend;
use crate::cmd::ec2_bench::naming::s3_key;

/// One poll observation.
#[derive(Debug, PartialEq, Eq)]
pub enum PollStep {
    /// No marker yet and the instance is still alive.
    Pending,
    /// The run finished successfully.
    Done,
    /// The run failed; carries the reported exit code.
    Failed(i64),
    /// The instance is gone with no marker (e.g. spot interruption).
    Lost,
}

/// A single poll round: read `status.json`, else check instance liveness.
pub async fn poll_once<A: AwsBackend>(
    aws: &A,
    bucket: &str,
    prefix: &str,
    instance_id: &str,
) -> anyhow::Result<PollStep> {
    let key = s3_key(prefix, "status.json");
    if let Some(text) = aws.get_object_text(bucket, &key).await? {
        let v: serde_json::Value = serde_json::from_str(&text)?;
        return Ok(match v["status"].as_str() {
            Some("done") => PollStep::Done,
            _ => PollStep::Failed(v["exitCode"].as_i64().unwrap_or(-1)),
        });
    }
    let state = aws.instance_state(instance_id).await?;
    if state == "terminated" || state == "shutting-down" {
        Ok(PollStep::Lost)
    } else {
        Ok(PollStep::Pending)
    }
}

/// Download every object under the prefix into `dest`; return `compare.md`.
pub async fn collect_results<A: AwsBackend>(
    aws: &A,
    bucket: &str,
    prefix: &str,
    dest: &Path,
) -> anyhow::Result<Option<String>> {
    crate::fs::create_dir_all(dest)?;
    for key in aws.list_objects(bucket, prefix).await? {
        let name = key.rsplit('/').next().unwrap_or(&key);
        aws.download_object(bucket, &key, &dest.join(name)).await?;
    }
    aws.get_object_text(bucket, &s3_key(prefix, "compare.md"))
        .await
}

/// Poll on an interval until a terminal step or the deadline. Thin loop over
/// `poll_once`. Fails when the deadline expires.
pub async fn poll_until_done<A: AwsBackend>(
    aws: &A,
    bucket: &str,
    prefix: &str,
    instance_id: &str,
    max_runtime: Duration,
) -> anyhow::Result<PollStep> {
    let start = std::time::Instant::now();
    loop {
        match poll_once(aws, bucket, prefix, instance_id).await? {
            PollStep::Pending => {}
            terminal => return Ok(terminal),
        }
        if start.elapsed() >= max_runtime {
            anyhow::bail!("max-runtime exceeded while waiting for the run");
        }
        tokio::time::sleep(Duration::from_secs(15)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::ec2_bench::aws::drive_ready;
    use crate::cmd::ec2_bench::aws::fake::FakeAws;

    #[test]
    fn reports_done_from_status_marker() {
        let aws = FakeAws::with_status(r#"{"schemaVersion":1,"status":"done","exitCode":0}"#);
        let step = drive_ready(poll_once(&aws, "b", "arity-bench/r1/", "i-0")).unwrap();
        assert_eq!(step, PollStep::Done);
    }

    #[test]
    fn reports_failed_with_exit_code() {
        let aws = FakeAws::with_status(r#"{"schemaVersion":1,"status":"failed","exitCode":7}"#);
        let step = drive_ready(poll_once(&aws, "b", "arity-bench/r1/", "i-0")).unwrap();
        assert_eq!(step, PollStep::Failed(7));
    }

    #[test]
    fn pending_while_running_without_marker() {
        let aws = FakeAws::default();
        aws.state.lock().unwrap().instances.push("i-0".into());
        let step = drive_ready(poll_once(&aws, "b", "arity-bench/r1/", "i-0")).unwrap();
        assert_eq!(step, PollStep::Pending);
    }

    #[test]
    fn lost_when_instance_terminated_without_marker() {
        let aws = FakeAws::default();
        aws.state.lock().unwrap().instance_state = Some("terminated".into());
        let step = drive_ready(poll_once(&aws, "b", "arity-bench/r1/", "i-0")).unwrap();
        assert_eq!(step, PollStep::Lost);
    }

    #[test]
    fn collect_returns_compare_md() {
        let aws = FakeAws::default();
        {
            let mut s = aws.state.lock().unwrap();
            s.objects
                .insert("compare.md".into(), "| bench | delta |".into());
        }
        let dir =
            std::env::temp_dir().join(format!("ec2bench-collect-test-{}", std::process::id()));
        let md = drive_ready(collect_results(&aws, "b", "arity-bench/r1/", &dir)).unwrap();
        assert_eq!(md.as_deref(), Some("| bench | delta |"));
        drop(std::fs::remove_dir_all(&dir));
    }
}
