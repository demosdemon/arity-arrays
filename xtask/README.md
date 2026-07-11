# xtask

Workspace benchmark tooling. Subcommands:

- `charts <run.json> [baseline.json]` — regenerate `docs/bench/` SVGs and README tables.
- `compare --head <run.json>... --base <run.json>...` — the A/B delta table CI posts.
- `ec2-bench` — run the A/B/B/A benchmark on a dedicated-CPU EC2 instance (feature-gated).

## `ec2-bench` (optional feature)

Provisions a CPU-tuned EC2 instance, runs the interleaved base/head/head/base
benchmark, uploads results to your S3 bucket, and tears everything down. Build
with the feature (off by default):

```
cargo run -p xtask --features ec2-bench -- ec2-bench run --s3-bucket my-bucket
```

### Prerequisites

- AWS credentials (ambient chain, or `--profile` / `--region`).
- The refs being benchmarked must be **pushed to the remote** — the instance clones it.
- `--repo-url` must be **publicly clonable** (the instance clones anonymously; no SSH key).
- The subnet needs **outbound internet** (default VPC works; else IGW/NAT).
- No inbound access is required or opened.

### Examples

```
# On-demand, HEAD vs origin/main, full precision:
xtask ec2-bench run --s3-bucket my-bucket

# Quick + spot (cheaper; may be interrupted):
xtask ec2-bench run --s3-bucket my-bucket --quick --spot

# Dedicated tenancy, pinned nightly, billing tags:
xtask ec2-bench run --s3-bucket my-bucket --tenancy dedicated \
  --toolchain nightly-2026-07-01 --tag Team=perf --tag CostCenter=42

# Bring your own instance profile (skips role creation):
xtask ec2-bench run --s3-bucket my-bucket --instance-profile my-profile

# See the plan without creating anything:
xtask ec2-bench run --s3-bucket my-bucket --dry-run

# Recover orphaned resources if the CLI died mid-run:
xtask ec2-bench teardown --run-id 20260709t143000z-a1b2c3 --region us-east-1
```

### Cost

Billed as normal EC2/S3 usage. Three guardrails cap spend: an instance-side hard
deadline (`--max-runtime`, default 120 quick / 360 full), the shutdown-on-finish
trap, and `teardown --run-id` for orphans. `--dry-run` shows an approximate cost.

`--keep` retains auto-created resources for post-run inspection, but only the
security group and the auto-created IAM role/profile — never the instance.
Teardown always terminates the instance, and the instance also self-terminates by
powering off (launched with shutdown-behavior=terminate), so `--keep` can never
leave it running.

### Debugging

The instance opens no inbound ports. Watch progress via the EC2 console output
(`aws ec2 get-console-output`) and the streamed `s3://<bucket>/<prefix>bench.log`;
`status.json` records the final outcome.

### IAM — caller policy (v1)

Attach to the identity running the CLI. The **base** policy is always required;
the **auto-create** statements are only needed when you do *not* pass
`--instance-profile`. This policy is a versioned interface — if a future version
adds actions, re-provision it.

> [!NOTE]
> Passing `--instance-profile <yours>` requires the caller to also hold
> `iam:PassRole` on that role's ARN. The `PassBenchRole` statement below only
> grants it for the auto-created `arity-ec2-bench-*` roles.

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "ReadAndResolve",
      "Effect": "Allow",
      "Action": [
        "sts:GetCallerIdentity",
        "ec2:Describe*",
        "ec2:GetConsoleOutput"
      ],
      "Resource": "*"
    },
    {
      "Sid": "ResolveAmi",
      "Effect": "Allow",
      "Action": "ssm:GetParameter",
      "Resource": "arn:aws:ssm:*:*:parameter/aws/service/canonical/*"
    },
    {
      "Sid": "ResultsBucket",
      "Effect": "Allow",
      "Action": ["s3:GetObject", "s3:ListBucket"],
      "Resource": [
        "arn:aws:s3:::<bucket>",
        "arn:aws:s3:::<bucket>/*"
      ]
    },
    {
      "Sid": "Launch",
      "Effect": "Allow",
      "Action": ["ec2:RunInstances", "ec2:CreateTags", "ec2:CreateSecurityGroup"],
      "Resource": "*"
    },
    {
      "Sid": "PassBenchRole",
      "Effect": "Allow",
      "Action": "iam:PassRole",
      "Resource": "arn:aws:iam::<account-id>:role/arity-ec2-bench-*"
    },
    {
      "Sid": "DestroyOwnedOnly",
      "Effect": "Allow",
      "Action": ["ec2:TerminateInstances", "ec2:DeleteSecurityGroup"],
      "Resource": "*",
      "Condition": {
        "StringEquals": { "ec2:ResourceTag/ManagedBy": "arity-xtask-ec2-bench" }
      }
    },
    {
      "Sid": "AutoCreateRole",
      "Effect": "Allow",
      "Action": [
        "iam:CreateRole", "iam:PutRolePolicy", "iam:TagRole",
        "iam:DeleteRole", "iam:DeleteRolePolicy",
        "iam:CreateInstanceProfile", "iam:AddRoleToInstanceProfile",
        "iam:TagInstanceProfile", "iam:RemoveRoleFromInstanceProfile",
        "iam:DeleteInstanceProfile"
      ],
      "Resource": [
        "arn:aws:iam::<account-id>:role/arity-ec2-bench-*",
        "arn:aws:iam::<account-id>:instance-profile/arity-ec2-bench-*"
      ]
    }
  ]
}
```

### IAM — instance role (auto-created)

The auto-created role trusts only `ec2.amazonaws.com` and carries a single
`s3:PutObject` scoped to `arn:aws:s3:::<bucket>/<prefix>*`. The instance
terminates itself by powering off (launched with shutdown-behavior=terminate),
so it needs no EC2 permissions.

### Maintenance

The AWS adapter is verified only by manual end-to-end runs against an
often-releasing SDK family. Re-run a real launch after any `aws-sdk-*` major
bump, and before first use each quarter.
