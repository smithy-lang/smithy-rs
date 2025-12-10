/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::error::{ErrorKind, SigningError};
use crate::key::PrivateKey;
use crate::policy::Policy;
use aws_smithy_types::DateTime;
use std::borrow::Cow;
use std::fmt;
use std::time::Duration;

/// CloudFront-specific base64 encoding.
/// Standard base64 with: `+` → `-`, `=` → `_`, `/` → `~`
fn cloudfront_base64(data: &[u8]) -> String {
    base64_simd::STANDARD
        .encode_to_string(data)
        .replace('+', "-")
        .replace('=', "_")
        .replace('/', "~")
}

const COOKIE_POLICY: &str = "CloudFront-Policy";
const COOKIE_SIGNATURE: &str = "CloudFront-Signature";
const COOKIE_KEY_PAIR_ID: &str = "CloudFront-Key-Pair-Id";
const COOKIE_EXPIRES: &str = "CloudFront-Expires";

#[derive(Debug, Clone)]
enum Expiration {
    DateTime(DateTime),
    Duration(Duration),
}

/// Request to sign a CloudFront URL or generate signed cookies.
#[derive(Debug, Clone)]
pub struct SigningRequest {
    pub(crate) resource_url: String,
    pub(crate) resource_pattern: Option<String>,
    pub(crate) key_pair_id: String,
    pub(crate) private_key: PrivateKey,
    pub(crate) expiration: DateTime,
    pub(crate) active_date: Option<DateTime>,
    pub(crate) ip_range: Option<String>,
}

impl SigningRequest {
    /// Creates a new builder for constructing a signing request.
    pub fn builder() -> SigningRequestBuilder {
        SigningRequestBuilder::default()
    }
}

/// Builder for [`SigningRequest`].
#[derive(Default, Debug)]
pub struct SigningRequestBuilder {
    resource_url: Option<String>,
    resource_pattern: Option<String>,
    key_pair_id: Option<String>,
    private_key: Option<PrivateKey>,
    expiration: Option<Expiration>,
    active_date: Option<DateTime>,
    ip_range: Option<String>,
    time_source: Option<aws_smithy_async::time::SharedTimeSource>,
}

impl SigningRequestBuilder {
    /// Sets the CloudFront resource URL to sign.
    pub fn resource_url(mut self, url: impl Into<String>) -> Self {
        self.resource_url = Some(url.into());
        self
    }

    /// Sets a wildcard pattern for the policy's `Resource` field.
    ///
    /// Use this when you want the signed URL to grant access to multiple resources
    /// matching a pattern. If not set, `resource_url` is used in the policy.
    ///
    /// Wildcards:
    /// - `*` matches zero or more characters
    /// - `?` matches exactly one character
    ///
    /// # Example
    /// ```ignore
    /// // Sign a specific URL but grant access to all files under /videos/
    /// SigningRequest::builder()
    ///     .resource_url("https://d111111abcdef8.cloudfront.net/videos/intro.mp4")
    ///     .resource_pattern("https://d111111abcdef8.cloudfront.net/videos/*")
    ///     // ...
    /// ```
    pub fn resource_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.resource_pattern = Some(pattern.into());
        self
    }

    /// Sets the CloudFront key pair ID.
    pub fn key_pair_id(mut self, id: impl Into<String>) -> Self {
        self.key_pair_id = Some(id.into());
        self
    }

    /// Sets the private key for signing.
    pub fn private_key(mut self, key: PrivateKey) -> Self {
        self.private_key = Some(key);
        self
    }

    /// Sets an absolute expiration time.
    pub fn expires_at(mut self, time: DateTime) -> Self {
        self.expiration = Some(Expiration::DateTime(time));
        self
    }

    /// Sets a relative expiration time from now.
    pub fn expires_in(mut self, duration: Duration) -> Self {
        self.expiration = Some(Expiration::Duration(duration));
        self
    }

    /// Sets an activation time (not-before date) for custom policy.
    pub fn active_at(mut self, time: DateTime) -> Self {
        self.active_date = Some(time);
        self
    }

    /// Sets an IP range restriction (CIDR notation) for custom policy.
    pub fn ip_range(mut self, cidr: impl Into<String>) -> Self {
        self.ip_range = Some(cidr.into());
        self
    }

    /// Builds the signing request.
    pub fn build(self) -> Result<SigningRequest, SigningError> {
        let resource_url = self
            .resource_url
            .ok_or_else(|| SigningError::invalid_input("resource_url is required"))?;

        let key_pair_id = self
            .key_pair_id
            .ok_or_else(|| SigningError::invalid_input("key_pair_id is required"))?;

        let private_key = self
            .private_key
            .ok_or_else(|| SigningError::invalid_input("private_key is required"))?;

        let expiration = self.expiration.ok_or_else(|| {
            SigningError::invalid_input("expiration is required (use expires_at or expires_in)")
        })?;

        let expiration = match expiration {
            Expiration::DateTime(dt) => dt,
            Expiration::Duration(dur) => {
                let time_source = self.time_source.unwrap_or_default();
                let now = DateTime::from(time_source.now());
                DateTime::from_secs(now.secs() + dur.as_secs() as i64)
            }
        };

        // Validate that activeDate is before expirationDate
        if let Some(active) = self.active_date {
            if active.secs() >= expiration.secs() {
                return Err(SigningError::invalid_input(
                    "active_at must be before expiration",
                ));
            }
        }

        Ok(SigningRequest {
            resource_url,
            resource_pattern: self.resource_pattern,
            key_pair_id,
            private_key,
            expiration,
            active_date: self.active_date,
            ip_range: self.ip_range,
        })
    }
}

/// A signed CloudFront URL.
#[derive(Debug, Clone)]
pub struct SignedUrl {
    url: url::Url,
}

impl SignedUrl {
    pub(crate) fn new(url: String) -> Result<Self, SigningError> {
        let url = url::Url::parse(&url).map_err(|e| {
            SigningError::new(
                ErrorKind::InvalidInput,
                Some(Box::new(e)),
                Some("failed to parse URL".into()),
            )
        })?;
        Ok(Self { url })
    }

    /// Returns the complete signed URL as a string.
    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }

    /// Returns a reference to the parsed URL.
    pub fn as_url(&self) -> &url::Url {
        &self.url
    }

    /// Consumes self and returns the parsed URL.
    pub fn into_url(self) -> url::Url {
        self.url
    }
}

impl fmt::Display for SignedUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.url)
    }
}

impl AsRef<str> for SignedUrl {
    fn as_ref(&self) -> &str {
        self.url.as_str()
    }
}

impl AsRef<url::Url> for SignedUrl {
    fn as_ref(&self) -> &url::Url {
        &self.url
    }
}

#[cfg(feature = "http-1x")]
impl TryFrom<SignedUrl> for http::Request<()> {
    type Error = http::Error;

    fn try_from(signed_url: SignedUrl) -> Result<Self, Self::Error> {
        http::Request::builder()
            .uri(signed_url.url.as_str())
            .body(())
    }
}

#[cfg(feature = "http-1x")]
impl TryFrom<&SignedUrl> for http::Request<()> {
    type Error = http::Error;

    fn try_from(signed_url: &SignedUrl) -> Result<Self, Self::Error> {
        http::Request::builder()
            .uri(signed_url.url.as_str())
            .body(())
    }
}

/// Signed cookies for CloudFront.
#[derive(Debug, Clone)]
pub struct SignedCookies {
    cookies: Vec<(Cow<'static, str>, String)>,
}

impl SignedCookies {
    pub(crate) fn new(cookies: Vec<(Cow<'static, str>, String)>) -> Self {
        Self { cookies }
    }

    /// Returns all cookies as name-value pairs.
    pub fn cookies(&self) -> &[(Cow<'static, str>, String)] {
        &self.cookies
    }

    /// Gets a specific cookie value by name.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.cookies
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Returns an iterator over cookies.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.cookies.iter().map(|(n, v)| (n.as_ref(), v.as_str()))
    }
}

// Internal signing implementation
impl SigningRequest {
    /// Returns true if this request should use a canned policy.
    /// Canned policy is used only when there's no resource_pattern, active_date, or ip_range.
    fn use_canned_policy(&self) -> bool {
        self.resource_pattern.is_none() && self.active_date.is_none() && self.ip_range.is_none()
    }

    pub(crate) fn sign_url(&self) -> Result<SignedUrl, SigningError> {
        let policy = self.build_policy()?;
        let policy_json = policy.to_json();
        let signature = self.private_key.sign(policy_json.as_bytes())?;
        let signature_b64 = cloudfront_base64(&signature);

        let separator = if self.resource_url.contains('?') {
            "&"
        } else {
            "?"
        };

        let signed_url = if self.use_canned_policy() {
            format!(
                "{}{}Expires={}&Signature={}&Key-Pair-Id={}",
                self.resource_url,
                separator,
                self.expiration.secs(),
                signature_b64,
                self.key_pair_id
            )
        } else {
            let policy_b64 = policy.to_cloudfront_base64();
            format!(
                "{}{}Policy={}&Signature={}&Key-Pair-Id={}",
                self.resource_url, separator, policy_b64, signature_b64, self.key_pair_id
            )
        };

        SignedUrl::new(signed_url)
    }

    pub(crate) fn sign_cookies(&self) -> Result<SignedCookies, SigningError> {
        let policy = self.build_policy()?;
        let policy_json = policy.to_json();
        let signature = self.private_key.sign(policy_json.as_bytes())?;
        let signature_b64 = cloudfront_base64(&signature);

        let cookies = if self.use_canned_policy() {
            vec![
                (
                    Cow::Borrowed(COOKIE_EXPIRES),
                    self.expiration.secs().to_string(),
                ),
                (Cow::Borrowed(COOKIE_SIGNATURE), signature_b64),
                (Cow::Borrowed(COOKIE_KEY_PAIR_ID), self.key_pair_id.clone()),
            ]
        } else {
            let policy_b64 = policy.to_cloudfront_base64();
            vec![
                (Cow::Borrowed(COOKIE_POLICY), policy_b64),
                (Cow::Borrowed(COOKIE_SIGNATURE), signature_b64),
                (Cow::Borrowed(COOKIE_KEY_PAIR_ID), self.key_pair_id.clone()),
            ]
        };

        Ok(SignedCookies::new(cookies))
    }

    fn build_policy(&self) -> Result<Policy, SigningError> {
        // Use resource_pattern for policy if set, otherwise use resource_url
        let policy_resource = self.resource_pattern.as_ref().unwrap_or(&self.resource_url);

        let mut builder = Policy::builder()
            .resource(policy_resource)
            .expires_at(self.expiration);

        if let Some(active) = self.active_date {
            builder = builder.starts_at(active);
        }
        if let Some(ref ip) = self.ip_range {
            builder = builder.ip_range(ip);
        }

        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_RSA_KEY: &[u8] = b"-----BEGIN RSA PRIVATE KEY-----
MIIBPAIBAAJBANW8WjQksUoX/7nwOfRDNt1XQpLCueHoXSt91MASMOSAqpbzZvXO
g2hW2gCFUIFUPCByMXPoeRe6iUZ5JtjepssCAwEAAQJBALR7ybwQY/lKTLKJrZab
D4BXCCt/7ZFbMxnftsC+W7UHef4S4qFW8oOOLeYfmyGZK1h44rXf2AIp4PndKUID
1zECIQD1suunYw5U22Pa0+2dFThp1VMXdVbPuf/5k3HT2/hSeQIhAN6yX0aT/N6G
gb1XlBKw6GQvhcM0fXmP+bVXV+RtzFJjAiAP+2Z2yeu5u1egeV6gdCvqPnUcNobC
FmA/NMcXt9xMSQIhALEMMJEFAInNeAIXSYKeoPNdkMPDzGnD3CueuCLEZCevAiEA
j+KnJ7pJkTvOzFwE8RfNLli9jf6/OhyYaLL4et7Ng5k=
-----END RSA PRIVATE KEY-----";

    #[test]
    fn test_sign_url_canned_policy() {
        let key = PrivateKey::from_pem(TEST_RSA_KEY).unwrap();
        let request = SigningRequest::builder()
            .resource_url("https://d111111abcdef8.cloudfront.net/image.jpg")
            .key_pair_id("APKAEXAMPLE")
            .private_key(key)
            .expires_at(DateTime::from_secs(1767290400))
            .build()
            .unwrap();

        let signed_url = request.sign_url().unwrap();
        let url = signed_url.as_str();

        assert!(url.contains("Expires=1767290400"));
        assert!(url.contains("Signature="));
        assert!(url.contains("Key-Pair-Id=APKAEXAMPLE"));
        assert!(!url.contains("Policy="));
    }

    #[test]
    fn test_sign_url_custom_policy() {
        let key = PrivateKey::from_pem(TEST_RSA_KEY).unwrap();
        let request = SigningRequest::builder()
            .resource_url("https://d111111abcdef8.cloudfront.net/*")
            .key_pair_id("APKAEXAMPLE")
            .private_key(key)
            .expires_at(DateTime::from_secs(1767290400))
            .active_at(DateTime::from_secs(1767200000))
            .build()
            .unwrap();

        let signed_url = request.sign_url().unwrap();
        let url = signed_url.as_str();

        assert!(url.contains("Policy="));
        assert!(url.contains("Signature="));
        assert!(url.contains("Key-Pair-Id=APKAEXAMPLE"));
        assert!(!url.contains("Expires="));
    }

    #[test]
    fn test_sign_url_with_existing_params() {
        let key = PrivateKey::from_pem(TEST_RSA_KEY).unwrap();
        let request = SigningRequest::builder()
            .resource_url("https://d111111abcdef8.cloudfront.net/image.jpg?size=large")
            .key_pair_id("APKAEXAMPLE")
            .private_key(key)
            .expires_at(DateTime::from_secs(1767290400))
            .build()
            .unwrap();

        let signed_url = request.sign_url().unwrap();
        let url = signed_url.as_str();

        assert!(url.contains("size=large"));
        assert!(url.contains("&Expires="));
    }

    #[test]
    fn test_sign_cookies_canned_policy() {
        let key = PrivateKey::from_pem(TEST_RSA_KEY).unwrap();
        let request = SigningRequest::builder()
            .resource_url("https://d111111abcdef8.cloudfront.net/*")
            .key_pair_id("APKAEXAMPLE")
            .private_key(key)
            .expires_at(DateTime::from_secs(1767290400))
            .build()
            .unwrap();

        let cookies = request.sign_cookies().unwrap();

        assert_eq!(cookies.get("CloudFront-Expires"), Some("1767290400"));
        assert!(cookies.get("CloudFront-Signature").is_some());
        assert_eq!(cookies.get("CloudFront-Key-Pair-Id"), Some("APKAEXAMPLE"));
        assert!(cookies.get("CloudFront-Policy").is_none());
    }

    #[test]
    fn test_sign_cookies_custom_policy() {
        let key = PrivateKey::from_pem(TEST_RSA_KEY).unwrap();
        let request = SigningRequest::builder()
            .resource_url("https://d111111abcdef8.cloudfront.net/*")
            .key_pair_id("APKAEXAMPLE")
            .private_key(key)
            .expires_at(DateTime::from_secs(1767290400))
            .ip_range("192.0.2.0/24")
            .build()
            .unwrap();

        let cookies = request.sign_cookies().unwrap();

        assert!(cookies.get("CloudFront-Policy").is_some());
        assert!(cookies.get("CloudFront-Signature").is_some());
        assert_eq!(cookies.get("CloudFront-Key-Pair-Id"), Some("APKAEXAMPLE"));
        assert!(cookies.get("CloudFront-Expires").is_none());
    }

    #[test]
    fn test_sign_url_with_resource_pattern() {
        let key = PrivateKey::from_pem(TEST_RSA_KEY).unwrap();
        let request = SigningRequest::builder()
            .resource_url("https://d111111abcdef8.cloudfront.net/videos/intro.mp4")
            .resource_pattern("https://d111111abcdef8.cloudfront.net/videos/*")
            .key_pair_id("APKAEXAMPLE")
            .private_key(key)
            .expires_at(DateTime::from_secs(1767290400))
            .build()
            .unwrap();

        let signed_url = request.sign_url().unwrap();
        let url = signed_url.as_str();

        // Should use custom policy format (Policy param) because resource_pattern is set
        assert!(url.contains("Policy="));
        assert!(url.contains("Signature="));
        assert!(url.contains("Key-Pair-Id=APKAEXAMPLE"));
        assert!(!url.contains("Expires="));
        // The actual URL should be the resource_url, not the pattern
        assert!(url.starts_with("https://d111111abcdef8.cloudfront.net/videos/intro.mp4?"));
    }

    #[test]
    fn test_sign_cookies_with_resource_pattern() {
        let key = PrivateKey::from_pem(TEST_RSA_KEY).unwrap();
        let request = SigningRequest::builder()
            .resource_url("https://d111111abcdef8.cloudfront.net/videos/*")
            .resource_pattern("https://d111111abcdef8.cloudfront.net/videos/*")
            .key_pair_id("APKAEXAMPLE")
            .private_key(key)
            .expires_at(DateTime::from_secs(1767290400))
            .build()
            .unwrap();

        let cookies = request.sign_cookies().unwrap();

        // Should use custom policy format because resource_pattern is set
        assert!(cookies.get("CloudFront-Policy").is_some());
        assert!(cookies.get("CloudFront-Signature").is_some());
        assert_eq!(cookies.get("CloudFront-Key-Pair-Id"), Some("APKAEXAMPLE"));
        assert!(cookies.get("CloudFront-Expires").is_none());
    }

    #[test]
    fn test_signed_url_accessors() {
        let key = PrivateKey::from_pem(TEST_RSA_KEY).unwrap();
        let request = SigningRequest::builder()
            .resource_url("https://d111111abcdef8.cloudfront.net/image.jpg")
            .key_pair_id("APKAEXAMPLE")
            .private_key(key)
            .expires_at(DateTime::from_secs(1767290400))
            .build()
            .unwrap();

        let signed_url = request.sign_url().unwrap();

        // Test as_str()
        let url_str = signed_url.as_str();
        assert!(url_str.starts_with("https://d111111abcdef8.cloudfront.net/image.jpg"));
        assert!(url_str.contains("Expires="));

        // Test as_url()
        let url_ref = signed_url.as_url();
        assert_eq!(url_ref.scheme(), "https");
        assert_eq!(url_ref.host_str(), Some("d111111abcdef8.cloudfront.net"));
        assert_eq!(url_ref.path(), "/image.jpg");

        // Test Display
        let displayed = format!("{}", signed_url);
        assert_eq!(displayed, url_str);

        // Test AsRef<str>
        let as_ref_str: &str = signed_url.as_ref();
        assert_eq!(as_ref_str, url_str);

        // Test AsRef<url::Url>
        let as_ref_url: &url::Url = signed_url.as_ref();
        assert_eq!(as_ref_url, url_ref);

        // Test into_url()
        let url = signed_url.into_url();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("d111111abcdef8.cloudfront.net"));
    }

    #[cfg(feature = "http-1x")]
    #[test]
    fn test_signed_url_to_http_request() {
        let key = PrivateKey::from_pem(TEST_RSA_KEY).unwrap();
        let request = SigningRequest::builder()
            .resource_url("https://d111111abcdef8.cloudfront.net/image.jpg")
            .key_pair_id("APKAEXAMPLE")
            .private_key(key)
            .expires_at(DateTime::from_secs(1767290400))
            .build()
            .unwrap();

        let signed_url = request.sign_url().unwrap();

        // Test TryFrom<SignedUrl>
        let http_req: http::Request<()> = signed_url.clone().try_into().unwrap();
        assert_eq!(http_req.method(), http::Method::GET);
        assert!(http_req.uri().to_string().contains("Expires="));

        // Test TryFrom<&SignedUrl>
        let http_req: http::Request<()> = (&signed_url).try_into().unwrap();
        assert_eq!(http_req.method(), http::Method::GET);
        assert!(http_req.uri().to_string().contains("Expires="));
    }
}
