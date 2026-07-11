//! The full `ec2-bench run` flow: preflight → provision → poll → collect →
//! teardown, generic over `AwsBackend`. Dry-run and the confirmation prompt are
//! handled by the CLI dispatcher before this is called; `run` assumes a
//! confirmed, non-dry launch.
#![expect(
    clippy::future_not_send,
    reason = "AwsBackend futures are driven on a current-thread runtime; Send is unnecessary"
)]

use std::time::Duration;

use anyhow::Context;

use crate::cmd::ec2_bench::RunConfig;
use crate::cmd::ec2_bench::aws::AwsBackend;
use crate::cmd::ec2_bench::naming::resource_name;
use crate::cmd::ec2_bench::poll::PollStep;
use crate::cmd::ec2_bench::poll::collect_results;
use crate::cmd::ec2_bench::poll::poll_until_done;
use crate::cmd::ec2_bench::provision::ProvisionInput;
use crate::cmd::ec2_bench::provision::provision;
use crate::cmd::ec2_bench::teardown::TeardownInput;
use crate::cmd::ec2_bench::teardown::teardown;
use crate::cmd::ec2_bench::userdata::render_user_data;

/// Environment resolved during preflight.
#[derive(Debug)]
#[expect(
    clippy::struct_field_names,
    reason = "each field holds an AWS resource id and mirrors the ami_id/vpc_id/subnet_id names in LaunchSpec/ProvisionInput and the AWS API; the shared _id suffix is intrinsic, not redundant"
)]
pub struct AwsEnv {
    /// Resolved AMI id.
    pub ami_id: String,
    /// Resolved VPC id.
    pub vpc_id: String,
    /// Resolved subnet id.
    pub subnet_id: String,
}

/// Verify credentials and that the bucket is reachable (its write permission is
/// only exercised later, on the instance), then resolve the AMI, VPC, and
/// subnet. Fails clearly when no default VPC or subnet exists and none was
/// supplied.
pub async fn preflight<A: AwsBackend>(aws: &A, cfg: &RunConfig) -> anyhow::Result<AwsEnv> {
    // Fail fast on missing/invalid credentials before creating any resources.
    aws.account_id().await?;
    aws.bucket_reachable(&cfg.s3_bucket).await?;
    let ami_id = match &cfg.ami_id {
        Some(a) => a.clone(),
        None => aws.resolve_ami(RunConfig::ssm_ami_param()).await?,
    };
    let vpc_id = match &cfg.vpc_id {
        Some(v) => v.clone(),
        None => aws
            .default_vpc_id()
            .await?
            .context("no default VPC; pass --vpc-id and --subnet-id")?,
    };
    let subnet_id = match &cfg.subnet_id {
        Some(s) => s.clone(),
        None => aws
            .first_subnet(&vpc_id)
            .await?
            .context("no subnet in the VPC; pass --subnet-id")?,
    };
    Ok(AwsEnv {
        ami_id,
        vpc_id,
        subnet_id,
    })
}

/// Return the instance-profile name to attach: the user's if provided, else an
/// auto-created role + profile with the deterministic
/// `arity-ec2-bench-<run-id>` name. `iam:PassRole` on that name pattern is the
/// highest-risk caller permission; it is only needed on this path.
async fn ensure_instance_profile<A: AwsBackend>(
    aws: &A,
    cfg: &RunConfig,
) -> anyhow::Result<String> {
    use crate::cmd::ec2_bench::iam::assume_role_policy;
    use crate::cmd::ec2_bench::iam::instance_role_policy;
    use crate::cmd::ec2_bench::provision::managed_tags;

    if let Some(p) = &cfg.instance_profile {
        return Ok(p.clone());
    }
    let name = resource_name(&cfg.run_id);
    let tags = managed_tags(cfg.run_id.as_str(), &cfg.tags);
    aws.create_bench_role(
        &name,
        &assume_role_policy(),
        &instance_role_policy(&cfg.s3_bucket, &cfg.s3_prefix),
        &tags,
    )
    .await?;
    aws.create_instance_profile(&name, &name, &tags).await?;
    Ok(name)
}

/// Execute a confirmed, non-dry run. Returns the process exit code. Teardown
/// is best-effort and never masks the run's own result.
///
/// # Panics
/// Panics only if the internal poll loop violates its invariant of never
/// yielding `Pending`.
pub async fn run<A: AwsBackend>(aws: &A, cfg: &RunConfig) -> anyhow::Result<()> {
    let env = preflight(aws, cfg).await?;

    // Past preflight (which is read-only), provisioning may create an IAM
    // role/profile, a security group, and an instance. Capture the result so
    // teardown ALWAYS runs before we return, even when provisioning fails
    // partway and leaves resources behind.
    let outcome = provision_and_poll(aws, cfg, &env).await;

    // Best-effort cleanup, whatever happened. Gate on what the CLI would have
    // created (not the provision result, which is unavailable on failure):
    // teardown discovers security groups by the RunId tag and deletes the IAM
    // role/profile by its deterministic name, so it removes whatever partial
    // state exists and never touches user-supplied resources.
    let td = TeardownInput {
        run_id: cfg.run_id.as_str().to_owned(),
        resource_name: resource_name(&cfg.run_id),
        delete_security_group: cfg.creates_security_group && !cfg.keep,
        delete_iam: cfg.creates_instance_profile && !cfg.keep,
    };
    if let Err(e) = teardown(aws, &td).await {
        eprintln!("warning: teardown incomplete: {e}");
    }

    match outcome? {
        PollStep::Done => {
            if let Some(md) =
                collect_results(aws, &cfg.s3_bucket, &cfg.s3_prefix, &cfg.output_dir).await?
            {
                print!("{md}");
            }
            eprintln!("results: s3://{}/{}", cfg.s3_bucket, cfg.s3_prefix);
            Ok(())
        }
        PollStep::Failed(code) => anyhow::bail!(
            "run failed on the instance (exit {code}); see s3://{}/{}bench.log",
            cfg.s3_bucket,
            cfg.s3_prefix
        ),
        PollStep::Lost => anyhow::bail!("instance was lost before completion (spot interruption?)"),
        PollStep::Pending => unreachable!("poll_until_done never returns Pending"),
    }
}

/// Ensure the instance profile, provision the tagged instance, and poll to a
/// terminal status. Extracted so `run` can guarantee teardown on any failure in
/// this window (IAM creation, provisioning, or polling).
async fn provision_and_poll<A: AwsBackend>(
    aws: &A,
    cfg: &RunConfig,
    env: &AwsEnv,
) -> anyhow::Result<PollStep> {
    let profile_name = ensure_instance_profile(aws, cfg).await?;
    let user_data = render_user_data(&cfg.user_data_params());
    let pin = ProvisionInput {
        run_id: cfg.run_id.as_str().to_owned(),
        vpc_id: env.vpc_id.clone(),
        subnet_id: env.subnet_id.clone(),
        ami_id: env.ami_id.clone(),
        instance_type: cfg.instance_type.clone(),
        tenancy_dedicated: matches!(
            cfg.tenancy,
            crate::cmd::ec2_bench::validate::Tenancy::Dedicated
        ),
        spot: cfg.spot,
        instance_profile_name: profile_name,
        user_data,
        user_tags: cfg.tags.clone(),
        security_group_id: cfg.security_group_id.clone(),
    };
    let provisioned = provision(aws, &pin).await?;
    eprintln!(
        "launched {} ({:?})",
        provisioned.instance_id, provisioned.public_ip
    );
    poll_until_done(
        aws,
        &cfg.s3_bucket,
        &cfg.s3_prefix,
        &provisioned.instance_id,
        Duration::from_secs(u64::from(cfg.max_runtime_min) * 60),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::ec2_bench::aws::drive_ready;
    use crate::cmd::ec2_bench::aws::fake::FakeAws;
    use crate::cmd::ec2_bench::test_config;

    #[test]
    fn happy_path_launches_polls_and_tears_down() {
        let cfg = test_config(|a| a.instance_profile = Some("prof".into()));
        let aws = FakeAws::with_status(r#"{"schemaVersion":1,"status":"done","exitCode":0}"#);
        drive_ready(run(&aws, &cfg)).unwrap();
        let s = aws.state.lock().unwrap();
        assert!(s.instances.is_empty(), "instance terminated");
        assert!(s.security_groups.is_empty(), "created SG deleted");
        assert_eq!(s.terminated.len(), 1);
    }

    #[test]
    fn failed_run_returns_one_and_still_tears_down() {
        let cfg = test_config(|a| a.instance_profile = Some("prof".into()));
        let aws = FakeAws::with_status(r#"{"schemaVersion":1,"status":"failed","exitCode":3}"#);
        drive_ready(run(&aws, &cfg)).unwrap_err();
        assert!(aws.state.lock().unwrap().instances.is_empty());
    }

    #[test]
    fn auto_creates_and_deletes_iam_when_no_profile_given() {
        let cfg = test_config(|_| {});
        let aws = FakeAws::with_status(r#"{"schemaVersion":1,"status":"done","exitCode":0}"#);
        drive_ready(run(&aws, &cfg)).unwrap();
        // Role was created then deleted by teardown.
        assert!(
            aws.state.lock().unwrap().roles.is_empty(),
            "auto-created role removed"
        );
    }

    #[test]
    fn provision_failure_still_tears_down_auto_created_resources() {
        let cfg = test_config(|_| {});
        let aws = FakeAws::default();
        aws.state.lock().unwrap().fail_run_instance = true;

        // The run surfaces the provisioning error...
        let result = drive_ready(run(&aws, &cfg));
        assert!(result.is_err(), "provision failure should propagate");

        // ...but the auto-created IAM role AND the created security group must
        // still have been torn down despite the failure.
        let s = aws.state.lock().unwrap();
        assert!(
            s.roles.is_empty(),
            "auto-created role removed despite failure"
        );
        assert!(
            s.security_groups.is_empty(),
            "created security group removed despite failure"
        );
        assert_eq!(
            s.instances.len(),
            0,
            "no instance survived run_instance's failure"
        );
    }
}
