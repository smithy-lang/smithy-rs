# aws-sdk-cloudfront-url-signer

A library for generating signed URLs and cookies for Amazon CloudFront private content.

This crate provides utilities to create cryptographically signed URLs and cookies that grant
time-limited access to private CloudFront distributions. It supports both RSA-SHA1 and
ECDSA-SHA256 signing algorithms with canned (simple) and custom (advanced) policies.

## Key Features

- **Signed URLs**: Generate URLs with embedded signatures for single-resource access
- **Signed Cookies**: Generate cookies for multi-resource access without URL modification
- **Canned Policies**: Simple time-based expiration
- **Custom Policies**: Advanced access control with activation dates, IP restrictions, and wildcards
- **Multiple Key Formats**: RSA (PKCS#1/PKCS#8) and ECDSA P-256 (PKCS#8) private keys

## When to Use Signed URLs vs Cookies

**Use signed URLs when:**
- Restricting access to individual files (e.g., a download link)
- Users are accessing content through a client that doesn't support cookies
- You want to share a link that works without additional setup

**Use signed cookies when:**
- Providing access to multiple restricted files (e.g., all files in a subscriber area)
- You don't want to change your existing URLs
- You're building a web application where cookies are naturally handled

## Basic Usage

### Signing a URL with Canned Policy

Canned policies provide simple time-based access control with just an expiration time:

```rust,ignore
use aws_sdk_cloudfront_url_signer::{sign_url, SigningRequest, PrivateKey};
use aws_smithy_types::DateTime;

// Load your CloudFront private key
let private_key = PrivateKey::from_pem(include_bytes!("private_key.pem"))?;

// Create a signing request
let request = SigningRequest::builder()
    .resource_url("https://d111111abcdef8.cloudfront.net/image.jpg")
    .key_pair_id("APKAEIBAERJR2EXAMPLE")
    .private_key(private_key)
    .expires_at(DateTime::from_secs(1767290400))  // Absolute expiration
    .build()?;

// Generate the signed URL
let signed_url = sign_url(request)?;
println!("Signed URL: {}", signed_url);
```

The resulting URL will include `Expires`, `Signature`, and `Key-Pair-Id` query parameters.

### Using Relative Expiration

Instead of an absolute timestamp, you can specify a duration from now:

```rust,ignore
use std::time::Duration;

let request = SigningRequest::builder()
    .resource_url("https://d111111abcdef8.cloudfront.net/video.mp4")
    .key_pair_id("APKAEIBAERJR2EXAMPLE")
    .private_key(private_key)
    .expires_in(Duration::from_secs(3600))  // Valid for 1 hour
    .build()?;
```

### Signing a URL with Custom Policy

Custom policies enable advanced access control with activation dates, IP restrictions, and wildcard patterns:

```rust,ignore
use aws_sdk_cloudfront_url_signer::{sign_url, SigningRequest, PrivateKey};
use aws_smithy_types::DateTime;

let private_key = PrivateKey::from_pem(include_bytes!("private_key.pem"))?;

let request = SigningRequest::builder()
    .resource_url("https://d111111abcdef8.cloudfront.net/videos/*")
    .key_pair_id("APKAEIBAERJR2EXAMPLE")
    .private_key(private_key)
    .expires_at(DateTime::from_secs(1767290400))
    .active_at(DateTime::from_secs(1767200000))  // Not valid before this time
    .ip_range("192.0.2.0/24")                    // Restrict to IP range
    .build()?;

let signed_url = sign_url(request)?;
```

Custom policy URLs include a `Policy` parameter (base64-encoded JSON) instead of `Expires`.

### Generating Signed Cookies

Signed cookies work similarly but return cookie name-value pairs:

```rust,ignore
use aws_sdk_cloudfront_url_signer::{sign_cookies, SigningRequest, PrivateKey};
use aws_smithy_types::DateTime;

let private_key = PrivateKey::from_pem(include_bytes!("private_key.pem"))?;

let request = SigningRequest::builder()
    .resource_url("https://d111111abcdef8.cloudfront.net/*")
    .key_pair_id("APKAEIBAERJR2EXAMPLE")
    .private_key(private_key)
    .expires_at(DateTime::from_secs(1767290400))
    .build()?;

let cookies = sign_cookies(request)?;

// Set cookies in your HTTP response
for (name, value) in cookies.iter() {
    println!("Set-Cookie: {}={}; Domain=d111111abcdef8.cloudfront.net; Secure; HttpOnly", name, value);
}
```

For canned policies, cookies include:
- `CloudFront-Expires`
- `CloudFront-Signature`
- `CloudFront-Key-Pair-Id`

For custom policies, cookies include:
- `CloudFront-Policy`
- `CloudFront-Signature`
- `CloudFront-Key-Pair-Id`

## Private Key Loading

### From PEM Bytes

Load a key from bytes (useful when loading from AWS Secrets Manager or environment variables):

```rust,ignore
use aws_sdk_cloudfront_url_signer::PrivateKey;

// From a byte slice
let key = PrivateKey::from_pem(include_bytes!("private_key.pem"))?;

// From a string
let pem_string = std::fs::read_to_string("private_key.pem")?;
let key = PrivateKey::from_pem(pem_string.as_bytes())?;
```

### From File (Async)

With the `rt-tokio` feature enabled, you can load keys directly from files:

```rust,ignore
use aws_sdk_cloudfront_url_signer::PrivateKey;

let key = PrivateKey::from_pem_file("private_key.pem").await?;
```

### Supported Key Formats

| Format | Header | Key Type |
|--------|--------|----------|
| PKCS#1 | `-----BEGIN RSA PRIVATE KEY-----` | RSA only |
| PKCS#8 | `-----BEGIN PRIVATE KEY-----` | RSA or ECDSA P-256 |

RSA keys use SHA-1 signatures (required by CloudFront for RSA). ECDSA P-256 keys use SHA-256 signatures.

## Policy Types

### Canned Policy

A canned policy is automatically used when you only specify an expiration time. It's simpler and produces shorter URLs:

```rust,ignore
let request = SigningRequest::builder()
    .resource_url("https://example.cloudfront.net/file.pdf")
    .key_pair_id("APKAEXAMPLE")
    .private_key(key)
    .expires_at(DateTime::from_secs(1767290400))
    .build()?;
```

### Custom Policy

A custom policy is used when you specify any of:
- `active_at()` - URL becomes valid at this time (not-before)
- `ip_range()` - Restrict access to an IPv4 CIDR range

```rust,ignore
let request = SigningRequest::builder()
    .resource_url("https://example.cloudfront.net/premium/*")
    .key_pair_id("APKAEXAMPLE")
    .private_key(key)
    .expires_at(DateTime::from_secs(1767290400))
    .active_at(DateTime::from_secs(1767200000))  // Triggers custom policy
    .ip_range("10.0.0.0/8")                      // Also triggers custom policy
    .build()?;
```

## Wildcard Patterns

Custom policies support wildcards in the resource URL:

- `*` matches zero or more characters
- `?` matches exactly one character

```rust,ignore
// Access to all files under /videos/
.resource_url("https://d111111abcdef8.cloudfront.net/videos/*")

// Access to all .mp4 files
.resource_url("https://d111111abcdef8.cloudfront.net/*.mp4")

// Access to files matching pattern
.resource_url("https://d111111abcdef8.cloudfront.net/video-?.mp4")
```

## Error Handling

All operations return `Result<T, SigningError>`:

```rust,ignore
use aws_sdk_cloudfront_url_signer::{sign_url, SigningRequest, PrivateKey, error::SigningError};

let result = sign_url(request);
match result {
    Ok(signed_url) => println!("Success: {}", signed_url),
    Err(e) => {
        eprintln!("Signing failed: {}", e);
        if let Some(source) = e.source() {
            eprintln!("Caused by: {}", source);
        }
    }
}
```

Common error scenarios:
- Invalid private key format
- Missing required fields (resource_url, key_pair_id, private_key, expiration)
- Cryptographic signing failures

## URLs with Existing Query Parameters

The library correctly handles URLs that already have query parameters:

```rust,ignore
let request = SigningRequest::builder()
    .resource_url("https://d111111abcdef8.cloudfront.net/video.mp4?quality=hd")
    .key_pair_id("APKAEXAMPLE")
    .private_key(key)
    .expires_at(DateTime::from_secs(1767290400))
    .build()?;

let signed_url = sign_url(request)?;
// Result: https://...?quality=hd&Expires=...&Signature=...&Key-Pair-Id=...
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `rt-tokio` | Enables async file loading with `PrivateKey::from_pem_file()` |

## CloudFront Setup

Before using this library, you need to:

1. Create a CloudFront key pair in the AWS Console or via CLI
2. Upload the public key to CloudFront
3. Create a key group containing your public key
4. Configure your CloudFront distribution to use the key group for restricted content
5. Keep the private key secure for use with this library

See the [CloudFront Developer Guide](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/private-content-signed-urls.html) for detailed setup instructions.

<!-- anchor_start:footer -->
This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) and the [smithy-rs](https://github.com/smithy-lang/smithy-rs) code generator.
<!-- anchor_end:footer -->
