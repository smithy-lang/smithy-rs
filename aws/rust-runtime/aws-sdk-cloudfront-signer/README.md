# aws-sdk-cloudfront-signer

A library for generating signed URLs and cookies for Amazon CloudFront private content.

This crate provides utilities to create cryptographically signed URLs and cookies that grant
time-limited access to private CloudFront distributions. It supports both RSA-SHA1 and
ECDSA-SHA1 signing algorithms with canned (simple) and custom (advanced) policies.

## Key Features

- **Signed URLs**: Generate URLs with embedded signatures for single-resource access
- **Signed Cookies**: Generate cookies for multi-resource access without URL modification
- **Canned Policies**: Simple time-based expiration
- **Custom Policies**: Advanced access control with activation dates, IP restrictions, and wildcards
- **Policy Reuse**: Sign once with a wildcard resource, apply to multiple URLs
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

## Feature Flags

| Feature | Description |
|---------|-------------|
| `rt-tokio` | Enables async file loading with `PrivateKey::from_pem_file()` |
| `http-1x` | Enables conversion to `http::Request` types |

## CloudFront Setup

Before using this library, you need to:

1. Create a CloudFront key pair in the AWS Console or via CLI
2. Upload the public key to CloudFront
3. Create a key group containing your public key
4. Configure your CloudFront distribution to use the key group for restricted content
5. Keep the private key secure for use with this library

See the [CloudFront Developer Guide](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/private-content-signed-urls.html) for detailed setup instructions.

## Basic Usage

### Signing a URL with Canned Policy

Canned policies provide simple time-based access control with just an expiration time:

```rust,ignore
use aws_sdk_cloudfront_signer::{sign_url, SigningRequest, PrivateKey};
use aws_smithy_types::DateTime;

let private_key = PrivateKey::from_pem(include_bytes!("private_key.pem"))?;

let signed = sign_url(&SigningRequest::builder()
    .resource("https://d111111abcdef8.cloudfront.net/image.jpg")
    .key_pair_id("APKAEIBAERJR2EXAMPLE")
    .private_key(private_key)
    .expires_at(DateTime::from_secs(1767290400))
    .build()?)?;

println!("Signed URL: {}", signed);
```

The resulting URL will include `Expires`, `Signature`, and `Key-Pair-Id` query parameters.

### Using Relative Expiration

Instead of an absolute timestamp, you can specify a duration from now:

```rust,ignore
use std::time::Duration;

let signed = sign_url(&SigningRequest::builder()
    .resource("https://d111111abcdef8.cloudfront.net/video.mp4")
    .key_pair_id("APKAEIBAERJR2EXAMPLE")
    .private_key(private_key)
    .expires_in(Duration::from_secs(3600))  // Valid for 1 hour
    .build()?)?;
```

### Signing a URL with Custom Policy

Custom policies are used when `active_at` or `ip_range` is provided:

```rust,ignore
use aws_sdk_cloudfront_signer::{sign_url, SigningRequest, PrivateKey};
use aws_smithy_types::DateTime;

let private_key = PrivateKey::from_pem(include_bytes!("private_key.pem"))?;

let signed = sign_url(&SigningRequest::builder()
    .resource("https://d111111abcdef8.cloudfront.net/video.mp4")
    .key_pair_id("APKAEIBAERJR2EXAMPLE")
    .private_key(private_key)
    .expires_at(DateTime::from_secs(1767290400))
    .active_at(DateTime::from_secs(1767200000))  // Not valid before this time
    .ip_range("192.0.2.0/24")                    // Restrict to IP range
    .build()?)?;
```

Custom policy URLs include a `Policy` parameter (base64-encoded JSON) instead of `Expires`.

### Generating Signed Cookies

Signed cookies work similarly but return cookie name-value pairs:

```rust,ignore
use aws_sdk_cloudfront_signer::{sign_cookies, SigningRequest, PrivateKey};
use aws_smithy_types::DateTime;

let private_key = PrivateKey::from_pem(include_bytes!("private_key.pem"))?;

let cookies = sign_cookies(&SigningRequest::builder()
    .resource("https://d111111abcdef8.cloudfront.net/*")
    .key_pair_id("APKAEIBAERJR2EXAMPLE")
    .private_key(private_key)
    .expires_at(DateTime::from_secs(1767290400))
    .build()?)?;

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

### Wildcard Policy Reuse

When signing with a wildcard resource and custom policy, you can reuse the signing
components across multiple URLs matching the pattern. This avoids re-signing for
each URL:

```rust,ignore
let signed = sign_url(&SigningRequest::builder()
    .resource("https://d111111abcdef8.cloudfront.net/videos/*")
    .key_pair_id("APKAEIBAERJR2EXAMPLE")
    .private_key(key)
    .expires_at(DateTime::from_secs(1767290400))
    .build()?)?;

let components = signed.components();
let url1 = components.apply_to("https://d111111abcdef8.cloudfront.net/videos/intro.mp4")?;
let url2 = components.apply_to("https://d111111abcdef8.cloudfront.net/videos/trailer.mp4")?;
```

Note: Wildcard resources automatically use custom policy. CloudFront reconstructs
canned policies from the request URL, so the wildcard in the signed policy wouldn't
match. The SDK detects `*` in the resource and uses custom policy automatically.

## Private Key Loading

### From PEM Bytes

Load a key from bytes (useful when loading from AWS Secrets Manager or environment variables):

```rust,ignore
use aws_sdk_cloudfront_signer::PrivateKey;

// From a byte slice
let key = PrivateKey::from_pem(include_bytes!("private_key.pem"))?;

// From a string
let pem_string = std::fs::read_to_string("private_key.pem")?;
let key = PrivateKey::from_pem(pem_string.as_bytes())?;
```

### From File (Async)

With the `rt-tokio` feature enabled, you can load keys directly from files:

```rust,ignore
use aws_sdk_cloudfront_signer::PrivateKey;

let key = PrivateKey::from_pem_file("private_key.pem").await?;
```

### Supported Key Formats

| Format | Header | Key Type |
|--------|--------|----------|
| PKCS#1 | `-----BEGIN RSA PRIVATE KEY-----` | RSA only |
| PKCS#8 | `-----BEGIN PRIVATE KEY-----` | RSA or ECDSA P-256 |

Both RSA and ECDSA keys use SHA-1 signatures (required by CloudFront).

## Policy Types

### Canned Policy

A canned policy is automatically used when you only specify an expiration time. It's
simpler and produces shorter URLs:

```rust,ignore
let signed = sign_url(&SigningRequest::builder()
    .resource("https://example.cloudfront.net/file.pdf")
    .key_pair_id("APKAEXAMPLE")
    .private_key(key)
    .expires_at(DateTime::from_secs(1767290400))
    .build()?)?;
```

### Custom Policy

A custom policy is used when you specify any of:
- `active_at()` - URL becomes valid at this time (not-before)
- `ip_range()` - Restrict access to an IPv4 CIDR range

```rust,ignore
let signed = sign_url(&SigningRequest::builder()
    .resource("https://example.cloudfront.net/premium/video.mp4")
    .key_pair_id("APKAEXAMPLE")
    .private_key(key)
    .expires_at(DateTime::from_secs(1767290400))
    .active_at(DateTime::from_secs(1767200000))
    .ip_range("10.0.0.0/8")
    .build()?)?;
```

## Wildcard Patterns

The `resource` parameter supports wildcards:

- `*` matches zero or more characters
- `?` matches exactly one character

Common patterns:
- `https://example.cloudfront.net/videos/*` - All files under /videos/
- `https://example.cloudfront.net/*.mp4` - All .mp4 files
- `https://example.cloudfront.net/video-?.mp4` - video-1.mp4, video-2.mp4, etc.
- `*` - All resources (use with caution)

## Using Signed URLs with HTTP Clients

The `SignedUrl` type provides multiple ways to access the signed URL:

```rust,ignore
let signed = sign_url(&request)?;

// As a string slice
let url_str: &str = signed.as_str();

// As a parsed url::Url
let url: &url::Url = signed.as_url();

// Convert to owned url::Url
let url: url::Url = signed.into_url();

// Display trait
println!("Signed URL: {}", signed);
```

### HTTP 1.x Integration

With the `http-1x` feature enabled, you can convert signed URLs directly to `http::Request`:

```rust,ignore
use http::Request;

let signed = sign_url(&request)?;
let http_request: Request<()> = signed.try_into()?;
```

## Error Handling

All operations return `Result<T, SigningError>`:

```rust,ignore
use aws_sdk_cloudfront_signer::{sign_url, error::SigningError};

match sign_url(&request) {
    Ok(signed) => println!("Success: {}", signed),
    Err(e) => {
        eprintln!("Signing failed: {}", e);
        if let Some(source) = std::error::Error::source(&e) {
            eprintln!("Caused by: {}", source);
        }
    }
}
```

Common error scenarios:
- Invalid private key format
- Missing required fields (resource, key_pair_id, private_key, expiration)
- `active_at` >= expiration
- Cryptographic signing failures

## URLs with Existing Query Parameters

The library correctly handles URLs that already have query parameters:

```rust,ignore
let signed = sign_url(&SigningRequest::builder()
    .resource("https://d111111abcdef8.cloudfront.net/video.mp4?quality=hd")
    .key_pair_id("APKAEXAMPLE")
    .private_key(key)
    .expires_at(DateTime::from_secs(1767290400))
    .build()?)?;
// Result: https://...?quality=hd&Expires=...&Signature=...&Key-Pair-Id=...
```

<!-- anchor_start:footer -->
This crate is part of the [AWS SDK for Rust](https://awslabs.github.io/aws-sdk-rust/) and the [smithy-rs](https://github.com/smithy-lang/smithy-rs) code generator.
<!-- anchor_end:footer -->
