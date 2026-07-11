//! Idempotent resource cleanup. Instances and security groups are discovered by
//! the `RunId` tag; the IAM role/profile is deleted by its deterministic name
//! (IAM list APIs cannot filter by tag). Adapters must treat "not found" as
//! success so re-running is safe.

use crate::cmd::ec2_bench::aws::AwsBackend;

/// What to tear down for a run.
#[derive(Debug)]
pub struct TeardownInput {
    /// Run id whose tagged resources to remove.
    pub run_id: String,
    /// Deterministic IAM role/profile name (`arity-ec2-bench-<run-id>`).
    pub resource_name: String,
    /// Delete the security group (only if the CLI created it).
    pub delete_security_group: bool,
    /// Delete the IAM role + profile (only if the CLI created them).
    pub delete_iam: bool,
}

/// Terminate instances, then optionally delete the SG and IAM role/profile.
/// Safe to call repeatedly. Adapters map not-found to success, so re-running
/// teardown is safe.
#[expect(
    clippy::future_not_send,
    reason = "AwsBackend futures are driven on a current-thread runtime; Send is unnecessary"
)]
pub async fn teardown<A: AwsBackend>(aws: &A, input: &TeardownInput) -> anyhow::Result<()> {
    for id in aws.find_instances_by_run(&input.run_id).await? {
        aws.terminate_instance(&id).await?;
    }
    if input.delete_security_group {
        for id in aws.find_security_groups_by_run(&input.run_id).await? {
            aws.delete_security_group(&id).await?;
        }
    }
    if input.delete_iam {
        aws.delete_bench_role_and_profile(&input.resource_name)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::ec2_bench::aws::drive_ready;
    use crate::cmd::ec2_bench::aws::fake::FakeAws;

    fn input(delete_sg: bool, delete_iam: bool) -> TeardownInput {
        TeardownInput {
            run_id: "r1".into(),
            resource_name: "arity-ec2-bench-r1".into(),
            delete_security_group: delete_sg,
            delete_iam,
        }
    }

    fn seeded() -> FakeAws {
        let aws = FakeAws::default();
        {
            let mut s = aws.state.lock().unwrap();
            s.instances.push("i-0".into());
            s.security_groups.push("sg-0".into());
            s.roles.push("arity-ec2-bench-r1".into());
        }
        aws
    }

    #[test]
    fn deletes_all_owned_resources() {
        let aws = seeded();
        drive_ready(teardown(&aws, &input(true, true))).unwrap();
        let s = aws.state.lock().unwrap();
        assert!(s.instances.is_empty());
        assert!(s.security_groups.is_empty());
        assert!(s.roles.is_empty());
        assert_eq!(s.terminated, vec!["i-0"]);
    }

    #[test]
    fn is_idempotent() {
        let aws = seeded();
        drive_ready(teardown(&aws, &input(true, true))).unwrap();
        // Second call finds nothing and still succeeds.
        drive_ready(teardown(&aws, &input(true, true))).unwrap();
    }

    #[test]
    fn skips_provided_sg_and_profile() {
        let aws = seeded();
        drive_ready(teardown(&aws, &input(false, false))).unwrap();
        let s = aws.state.lock().unwrap();
        assert!(s.instances.is_empty(), "instance always terminated");
        assert_eq!(s.security_groups, vec!["sg-0"], "provided SG untouched");
        assert_eq!(
            s.roles,
            vec!["arity-ec2-bench-r1"],
            "provided profile untouched"
        );
    }
}
