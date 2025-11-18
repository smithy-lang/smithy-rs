/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::error::SigningError;
use crate::key::PrivateKey;
use aws_smithy_types::DateTime;
use std::time::Duration;

#[derive(Debug, Clone)]
enum Expiration {
    DateTime(DateTime),
    Duration(Duration),
}

#[derive(Debug, Clone)]
struct CustomPolicyOptions {
    active_date: Option<DateTime>,
    ip_range: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SigningRequest {
    pub(crate) resource_url: String,
    pub(crate) key_pair_id: String,
    pub(crate) private_key: PrivateKey,
    pub(crate) expiration: DateTime,
    pub(crate) custom_policy_options: Option<CustomPolicyOptions>,
}

impl SigningRequest {
    pub fn builder() -> SigningRequestBuilder {
        SigningRequestBuilder::default()
    }

    pub(crate) fn is_custom_policy(&self) -> bool {
        self.custom_policy_options.is_some()
    }
}

#[derive(Default)]
pub struct SigningRequestBuilder {
    resource_url: Option<String>,
    key_pair_id: Option<String>,
    private_key: Option<PrivateKey>,
    expiration: Option<Expiration>,
    active_date: Option<DateTime>,
    ip_range: Option<String>,
}

impl SigningRequestBuilder {
    pub fn resource_url(mut self, url: impl Into<String>) -> Self {
        self.resource_url = Some(url.into());
        self
    }

    pub fn key_pair_id(mut self, id: impl Into<String>) -> Self {
        self.key_pair_id = Some(id.into());
        self
    }

    pub fn private_key(mut self, key: PrivateKey) -> Self {
        self.private_key = Some(key);
        self
    }

    pub fn expires_at(mut self, time: DateTime) -> Self {
        self.expiration = Some(Expiration::DateTime(time));
        self
    }

    pub fn expires_in(mut self, duration: Duration) -> Self {
        self.expiration = Some(Expiration::Duration(duration));
        self
    }

    pub fn active_at(mut self, time: DateTime) -> Self {
        self.active_date = Some(time);
        self
    }

    pub fn ip_range(mut self, cidr: impl Into<String>) -> Self {
        self.ip_range = Some(cidr.into());
        self
    }

    pub fn build(self) -> Result<SigningRequest, SigningError> {
        let resource_url = self
            .resource_url
            .ok_or_else(|| SigningError::InvalidInput {
                message: "resource_url is required".to_string(),
            })?;

        let key_pair_id = self.key_pair_id.ok_or_else(|| SigningError::InvalidInput {
            message: "key_pair_id is required".to_string(),
        })?;

        let private_key = self.private_key.ok_or_else(|| SigningError::InvalidInput {
            message: "private_key is required".to_string(),
        })?;

        let expiration = self.expiration.ok_or_else(|| SigningError::InvalidInput {
            message: "expiration is required (use expires_at or expires_in)".to_string(),
        })?;

        let expiration = match expiration {
            Expiration::DateTime(dt) => dt,
            Expiration::Duration(dur) => {
                let now = DateTime::from_secs(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64,
                );
                DateTime::from_secs(now.secs() + dur.as_secs() as i64)
            }
        };

        let custom_policy_options = if self.active_date.is_some() || self.ip_range.is_some() {
            Some(CustomPolicyOptions {
                active_date: self.active_date,
                ip_range: self.ip_range,
            })
        } else {
            None
        };

        Ok(SigningRequest {
            resource_url,
            key_pair_id,
            private_key,
            expiration,
            custom_policy_options,
        })
    }
}
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt;

#[derive(Debug, Clone)]
pub struct SignedUrl {
    url: String,
}

impl SignedUrl {
    pub(crate) fn new(url: String) -> Self {
        Self { url }
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl fmt::Display for SignedUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.url)
    }
}

#[derive(Debug, Clone)]
pub struct SignedCookies {
    cookies: Vec<(String, String)>,
}

impl SignedCookies {
    pub(crate) fn new(cookies: Vec<(String, String)>) -> Self {
        Self { cookies }
    }

    pub fn cookies(&self) -> &[(String, String)] {
        &self.cookies
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.cookies
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.cookies.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }
}
