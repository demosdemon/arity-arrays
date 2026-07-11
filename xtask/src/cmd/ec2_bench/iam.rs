//! IAM policy documents for the auto-created instance role. The instance
//! carries only the permission it needs to upload results; the caller policy
//! itself lives in `xtask/README.md` as copy-paste JSON.

use serde_json::json;

/// Least-privilege permissions policy for the instance role: a single
/// `s3:PutObject` scoped to the run's bucket/prefix.
#[must_use]
pub fn instance_role_policy(bucket: &str, prefix: &str) -> String {
    let resource = format!("arn:aws:s3:::{bucket}/{prefix}*");
    json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Action": "s3:PutObject",
            "Resource": resource,
        }],
    })
    .to_string()
}

/// Trust policy allowing only the EC2 service to assume the instance role.
#[must_use]
pub fn assume_role_policy() -> String {
    json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": { "Service": "ec2.amazonaws.com" },
            "Action": "sts:AssumeRole",
        }],
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_policy_scopes_put_object_to_prefix() {
        let doc = instance_role_policy("my-bucket", "arity-bench/run/");
        let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
        assert_eq!(v["Statement"][0]["Action"], "s3:PutObject");
        assert_eq!(
            v["Statement"][0]["Resource"],
            "arn:aws:s3:::my-bucket/arity-bench/run/*"
        );
        assert_eq!(v["Statement"][0]["Effect"], "Allow");
    }

    #[test]
    fn instance_policy_appends_star_without_double_slash() {
        // A prefix without a trailing slash still yields one wildcard segment.
        let doc = instance_role_policy("b", "p");
        let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
        assert_eq!(v["Statement"][0]["Resource"], "arn:aws:s3:::b/p*");
    }

    #[test]
    fn trust_policy_allows_only_ec2() {
        let doc = assume_role_policy();
        let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
        assert_eq!(
            v["Statement"][0]["Principal"]["Service"],
            "ec2.amazonaws.com"
        );
        assert_eq!(v["Statement"][0]["Action"], "sts:AssumeRole");
    }
}
