/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::error::SigningError;
use aws_smithy_types::{DateTime, Number};

#[derive(Debug, Clone)]
pub(crate) struct Policy {
    resource: String,
    expires_at: i64,
    starts_at: Option<i64>,
    ip_range: Option<String>,
}

impl Policy {
    pub(crate) fn builder() -> PolicyBuilder {
        PolicyBuilder::default()
    }

    pub(crate) fn is_canned(&self) -> bool {
        self.starts_at.is_none() && self.ip_range.is_none()
    }

    pub(crate) fn to_json(&self) -> String {
        let mut out = String::new();
        let mut root = aws_smithy_json::serialize::JsonObjectWriter::new(&mut out);

        let mut statement_array = root.key("Statement").start_array();
        let mut statement = statement_array.value().start_object();

        statement.key("Resource").string(&self.resource);

        let mut condition = statement.key("Condition").start_object();

        let mut date_less = condition.key("DateLessThan").start_object();
        date_less
            .key("AWS:EpochTime")
            .number(Number::PosInt(self.expires_at as u64));
        date_less.finish();

        if let Some(starts) = self.starts_at {
            let mut date_greater = condition.key("DateGreaterThan").start_object();
            date_greater
                .key("AWS:EpochTime")
                .number(Number::PosInt(starts as u64));
            date_greater.finish();
        }

        if let Some(ref ip) = self.ip_range {
            let mut ip_addr = condition.key("IpAddress").start_object();
            ip_addr.key("AWS:SourceIp").string(ip);
            ip_addr.finish();
        }

        condition.finish();
        statement.finish();
        statement_array.finish();
        root.finish();

        out
    }

    pub(crate) fn to_base64url(&self) -> String {
        let json = self.to_json();
        base64_simd::URL_SAFE_NO_PAD.encode_to_string(json.as_bytes())
    }
}

#[derive(Default)]
pub(crate) struct PolicyBuilder {
    resource: Option<String>,
    expires_at: Option<DateTime>,
    starts_at: Option<DateTime>,
    ip_range: Option<String>,
}

impl PolicyBuilder {
    pub(crate) fn resource(mut self, url: impl Into<String>) -> Self {
        self.resource = Some(url.into());
        self
    }

    pub(crate) fn expires_at(mut self, time: DateTime) -> Self {
        self.expires_at = Some(time);
        self
    }

    pub(crate) fn starts_at(mut self, time: DateTime) -> Self {
        self.starts_at = Some(time);
        self
    }

    pub(crate) fn ip_range(mut self, cidr: impl Into<String>) -> Self {
        self.ip_range = Some(cidr.into());
        self
    }

    pub(crate) fn build(self) -> Result<Policy, SigningError> {
        let resource = self.resource.ok_or_else(|| SigningError::InvalidPolicy {
            message: "resource is required".to_string(),
        })?;

        let expires_at = self.expires_at.ok_or_else(|| SigningError::InvalidPolicy {
            message: "expires_at is required".to_string(),
        })?;

        let expires_epoch = expires_at.secs();
        let starts_epoch = self.starts_at.map(|dt| dt.secs());

        if let Some(starts) = starts_epoch {
            if starts >= expires_epoch {
                return Err(SigningError::InvalidPolicy {
                    message: "starts_at must be before expires_at".to_string(),
                });
            }
        }

        Ok(Policy {
            resource,
            expires_at: expires_epoch,
            starts_at: starts_epoch,
            ip_range: self.ip_range,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canned_policy() {
        let policy = Policy::builder()
            .resource("https://d111111abcdef8.cloudfront.net/image.jpg")
            .expires_at(DateTime::from_secs(1767290400))
            .build()
            .expect("valid canned policy");

        assert!(policy.is_canned());
        let json = policy.to_json();
        assert!(json.contains("\"Resource\":\"https://d111111abcdef8.cloudfront.net/image.jpg\""));
        assert!(json.contains("\"AWS:EpochTime\":1767290400"));
        assert!(!json.contains("DateGreaterThan"));
        assert!(!json.contains("IpAddress"));
    }

    #[test]
    fn test_custom_policy_with_starts_at() {
        let policy = Policy::builder()
            .resource("https://d111111abcdef8.cloudfront.net/*")
            .expires_at(DateTime::from_secs(1767290400))
            .starts_at(DateTime::from_secs(1767200000))
            .build()
            .expect("valid custom policy");

        assert!(!policy.is_canned());
        let json = policy.to_json();
        assert!(json.contains("DateGreaterThan"));
        assert!(json.contains("\"AWS:EpochTime\":1767200000"));
    }

    #[test]
    fn test_custom_policy_with_ip_range() {
        let policy = Policy::builder()
            .resource("https://d111111abcdef8.cloudfront.net/video.mp4")
            .expires_at(DateTime::from_secs(1767290400))
            .ip_range("192.0.2.0/24")
            .build()
            .expect("valid custom policy");

        assert!(!policy.is_canned());
        let json = policy.to_json();
        assert!(json.contains("IpAddress"));
        assert!(json.contains("\"AWS:SourceIp\":\"192.0.2.0/24\""));
    }

    #[test]
    fn test_missing_resource() {
        let result = Policy::builder()
            .expires_at(DateTime::from_secs(1767290400))
            .build();

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SigningError::InvalidPolicy { .. }
        ));
    }

    #[test]
    fn test_missing_expires_at() {
        let result = Policy::builder()
            .resource("https://example.com/file.txt")
            .build();

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SigningError::InvalidPolicy { .. }
        ));
    }

    #[test]
    fn test_starts_at_after_expires_at() {
        let result = Policy::builder()
            .resource("https://example.com/file.txt")
            .expires_at(DateTime::from_secs(1767200000))
            .starts_at(DateTime::from_secs(1767290400))
            .build();

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SigningError::InvalidPolicy { .. }
        ));
    }

    #[test]
    fn test_base64url_encoding() {
        let policy = Policy::builder()
            .resource("https://example.com/test")
            .expires_at(DateTime::from_secs(1767290400))
            .build()
            .expect("valid policy");

        let encoded = policy.to_base64url();
        assert!(!encoded.contains('='));
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
    }
}
