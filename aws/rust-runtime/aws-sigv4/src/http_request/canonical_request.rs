/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use super::{Error, PayloadChecksumKind, SignableBody, SigningSettings, UriEncoding};
use crate::date_fmt::{format_date, format_date_time, parse_date, parse_date_time};
use crate::http_request::sign::SignableRequest;
use crate::http_request::url_escape::percent_encode;
use crate::sign::sha256_hex_string;
use chrono::{Date, DateTime, Utc};
use http::header::{HeaderName, HOST, USER_AGENT};
use http::{HeaderMap, HeaderValue, Method, Uri};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::convert::TryFrom;
use std::fmt;
use std::fmt::Formatter;

pub(crate) const HMAC_256: &str = "AWS4-HMAC-SHA256";
pub(crate) const X_AMZ_SECURITY_TOKEN: &str = "x-amz-security-token";
pub(crate) const X_AMZ_DATE: &str = "x-amz-date";
pub(crate) const X_AMZ_CONTENT_SHA_256: &str = "x-amz-content-sha256";

const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

#[derive(Debug, PartialEq)]
pub struct CanonicalRequest<'a> {
    pub method: &'a Method,
    pub path: String,
    pub params: Option<String>,
    pub headers: HeaderMap,
    pub signed_headers: SignedHeaders,
    pub date_time: String,
    pub security_token: Option<&'a str>,
    pub content_sha256: Cow<'a, str>,
}

impl<'a> CanonicalRequest<'a> {
    /// Construct a CanonicalRequest from an HttpRequest and a signable body
    ///
    /// This function returns 2 things:
    /// 1. The canonical request to use for signing
    /// 2. `AddedHeaders`, a struct recording the additional headers that were added. These will
    ///    behavior returned to the top level caller. If the caller wants to create a
    ///    presigned URL, they can apply these parameters to the query string.
    ///
    /// ## Behavior
    /// There are several settings which alter signing behavior:
    /// - If a `security_token` is provided as part of the credentials it will be included in the signed headers
    /// - If `settings.uri_encoding` specifies double encoding, `%` in the URL will be rencoded as
    /// `%25`
    /// - If settings.payload_checksum_kind is XAmzSha256, add a x-amz-content-sha256 with the body
    /// checksum. This is the same checksum used as the "payload_hash" in the canonical request
    pub fn from<'b>(
        req: &'b SignableRequest<'b>,
        settings: &SigningSettings,
        date: DateTime<Utc>,
        security_token: Option<&'b str>,
    ) -> Result<CanonicalRequest<'b>, Error> {
        // Path encoding: if specified, re-encode % as %25
        // Set method and path into CanonicalRequest
        let path = req.uri().path();
        let path = match settings.uri_encoding {
            // The string is already URI encoded, we don't need to encode everything again, just `%`
            UriEncoding::Double => path.replace('%', "%25"),
            UriEncoding::Single => path.to_string(),
        };
        let payload_hash = Self::payload_hash(req.body());

        let date_time = format_date_time(&date);
        let (signed_headers, canonical_headers) =
            Self::headers(req, settings, &payload_hash, &date_time, security_token)?;
        let creq = CanonicalRequest {
            method: req.method(),
            path,
            params: Self::params(req.uri()),
            headers: canonical_headers,
            signed_headers: SignedHeaders::new(signed_headers),
            date_time,
            security_token,
            content_sha256: payload_hash,
        };
        Ok(creq)
    }

    fn headers(
        req: &SignableRequest,
        settings: &SigningSettings,
        payload_hash: &str,
        date_time: &str,
        security_token: Option<&str>,
    ) -> Result<(Vec<CanonicalHeaderName>, HeaderMap), Error> {
        // Header computation:
        // The canonical request will include headers not present in the input. We need to clone
        // the headers from the original request and add:
        // - host
        // - x-amz-date
        // - x-amz-security-token (if provided)
        // - x-amz-content-sha256 (if requested by signing settings)
        let mut canonical_headers = req.headers().clone();
        Self::insert_host_header(&mut canonical_headers, req.uri());
        Self::insert_date_header(&mut canonical_headers, &date_time);

        if let Some(security_token) = security_token {
            let mut sec_header = HeaderValue::from_str(security_token)?;
            sec_header.set_sensitive(true);
            canonical_headers.insert(X_AMZ_SECURITY_TOKEN, sec_header);
        }

        if settings.payload_checksum_kind == PayloadChecksumKind::XAmzSha256 {
            let header = HeaderValue::from_str(&payload_hash)?;
            canonical_headers.insert(X_AMZ_CONTENT_SHA_256, header);
        }

        let mut signed_headers = Vec::with_capacity(canonical_headers.len());
        for (name, _) in &canonical_headers {
            // The user agent header should not be signed because it may be altered by proxies
            if name != USER_AGENT {
                signed_headers.push(CanonicalHeaderName(name.clone()));
            }
        }
        Ok((signed_headers, canonical_headers))
    }

    fn payload_hash<'b>(body: &'b SignableBody<'b>) -> Cow<'b, str> {
        // Payload hash computation
        //
        // Based on the input body, set the payload_hash of the canonical request:
        // Either:
        // - compute a hash
        // - use the precomputed hash
        // - use `UnsignedPayload`
        match body {
            SignableBody::Bytes(data) => Cow::Owned(sha256_hex_string(data)),
            SignableBody::Precomputed(digest) => Cow::Borrowed(digest.as_str()),
            SignableBody::UnsignedPayload => Cow::Borrowed(UNSIGNED_PAYLOAD),
        }
    }

    fn params(uri: &Uri) -> Option<String> {
        if let Some(query) = uri.query() {
            let mut first = true;
            let mut out = String::new();
            let mut params: Vec<(Cow<str>, Cow<str>)> =
                form_urlencoded::parse(query.as_bytes()).collect();
            // Sort by param name, and then by param value
            params.sort();
            for (key, value) in params {
                if !first {
                    out.push('&');
                }
                first = false;

                out.push_str(&percent_encode(&key));
                out.push('=');
                out.push_str(&percent_encode(&value));
            }
            Some(out)
        } else {
            None
        }
    }

    fn insert_host_header(
        canonical_headers: &mut HeaderMap<HeaderValue>,
        uri: &Uri,
    ) -> HeaderValue {
        match canonical_headers.get(&HOST) {
            Some(header) => header.clone(),
            None => {
                let authority = uri
                    .authority()
                    .expect("request uri authority must be set for signing");
                let header = HeaderValue::try_from(authority.as_str())
                    .expect("endpoint must contain valid header characters");
                canonical_headers.insert(HOST, header.clone());
                header
            }
        }
    }

    fn insert_date_header(
        canonical_headers: &mut HeaderMap<HeaderValue>,
        date_time: &str,
    ) -> HeaderValue {
        let x_amz_date = HeaderName::from_static(X_AMZ_DATE);
        let date_header = HeaderValue::try_from(date_time).expect("date is valid header value");
        canonical_headers.insert(x_amz_date, date_header.clone());
        date_header
    }
}

impl<'a> fmt::Display for CanonicalRequest<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.method)?;
        writeln!(f, "{}", self.path)?;
        writeln!(f, "{}", self.params.as_deref().unwrap_or(""))?;
        // write out _all_ the headers
        for header in &self.signed_headers.inner {
            // a missing header is a bug, so we should panic.
            let value = &self.headers[&header.0];
            write!(f, "{}:", header.0.as_str())?;
            writeln!(f, "{}", value.to_str().unwrap())?;
        }
        writeln!(f)?;
        // write out the signed headers
        write!(f, "{}", self.signed_headers.to_string())?;
        writeln!(f)?;
        write!(f, "{}", self.content_sha256)?;
        Ok(())
    }
}

#[derive(Debug, PartialEq, Default)]
pub struct SignedHeaders {
    inner: Vec<CanonicalHeaderName>,
}

impl SignedHeaders {
    fn new(mut inner: Vec<CanonicalHeaderName>) -> Self {
        inner.sort();
        SignedHeaders { inner }
    }
}

impl fmt::Display for SignedHeaders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut iter = self.inner.iter().peekable();
        while let Some(next) = iter.next() {
            match iter.peek().is_some() {
                true => write!(f, "{};", next.0.as_str())?,
                false => write!(f, "{}", next.0.as_str())?,
            };
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CanonicalHeaderName(HeaderName);

impl PartialOrd for CanonicalHeaderName {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CanonicalHeaderName {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.as_str().cmp(&other.0.as_str())
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct Scope<'a> {
    pub date: Date<Utc>,
    pub region: &'a str,
    pub service: &'a str,
}

impl<'a> fmt::Display for Scope<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}/{}/aws4_request",
            format_date(&self.date),
            self.region,
            self.service
        )
    }
}

impl<'a> TryFrom<&'a str> for Scope<'a> {
    type Error = Error;
    fn try_from(s: &'a str) -> Result<Scope<'a>, Self::Error> {
        let mut scopes = s.split('/');
        let date = parse_date(scopes.next().expect("missing date"))?;
        let region = scopes.next().expect("missing region");
        let service = scopes.next().expect("missing service");

        let scope = Scope {
            date,
            region,
            service,
        };

        Ok(scope)
    }
}

#[derive(PartialEq, Debug)]
pub struct StringToSign<'a> {
    pub scope: Scope<'a>,
    pub date: DateTime<Utc>,
    pub region: &'a str,
    pub service: &'a str,
    pub hashed_creq: &'a str,
}

impl<'a> TryFrom<&'a str> for StringToSign<'a> {
    type Error = Error;
    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        let lines = s.lines().collect::<Vec<&str>>();
        let date = parse_date_time(&lines[1])?;
        let scope: Scope = TryFrom::try_from(lines[2])?;
        let hashed_creq = &lines[3];

        let sts = StringToSign {
            date,
            region: scope.region,
            service: scope.service,
            scope,
            hashed_creq,
        };

        Ok(sts)
    }
}

impl<'a> StringToSign<'a> {
    pub(crate) fn new(
        date: DateTime<Utc>,
        region: &'a str,
        service: &'a str,
        hashed_creq: &'a str,
    ) -> Self {
        let scope = Scope {
            date: date.date(),
            region,
            service,
        };
        Self {
            scope,
            date,
            region,
            service,
            hashed_creq,
        }
    }
}

impl<'a> fmt::Display for StringToSign<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}\n{}\n{}\n{}",
            HMAC_256,
            format_date_time(&self.date),
            self.scope.to_string(),
            self.hashed_creq
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::date_fmt::parse_date_time;
    use crate::http_request::canonical_request::{CanonicalRequest, Scope, StringToSign};
    use crate::http_request::test::{test_canonical_request, test_request, test_sts};
    use crate::http_request::{
        PayloadChecksumKind, SignableBody, SignableRequest, SigningSettings,
    };
    use crate::sign::sha256_hex_string;
    use pretty_assertions::assert_eq;
    use std::convert::TryFrom;

    #[test]
    fn test_set_xamz_sha_256() {
        let req = test_request("get-vanilla-query-order-key-case");
        let req = SignableRequest::from_http(&req);
        let date = parse_date_time("20150830T123600Z").unwrap();
        let mut signing_settings = SigningSettings {
            payload_checksum_kind: PayloadChecksumKind::XAmzSha256,
            ..Default::default()
        };
        let creq = CanonicalRequest::from(&req, &signing_settings, date, None).unwrap();
        assert_eq!(
            &creq.content_sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // assert that the sha256 header was added
        assert_eq!(
            creq.signed_headers.to_string(),
            "host;x-amz-content-sha256;x-amz-date"
        );

        signing_settings.payload_checksum_kind = PayloadChecksumKind::NoHeader;
        let creq = CanonicalRequest::from(&req, &signing_settings, date, None).unwrap();
        assert_eq!(creq.signed_headers.to_string(), "host;x-amz-date");
    }

    #[test]
    fn test_unsigned_payload() {
        let req = test_request("get-vanilla-query-order-key-case");
        let req = SignableRequest::new(
            req.method(),
            req.uri(),
            req.headers(),
            SignableBody::UnsignedPayload,
        );
        let date = parse_date_time("20150830T123600Z").unwrap();
        let signing_settings = SigningSettings {
            payload_checksum_kind: PayloadChecksumKind::XAmzSha256,
            ..Default::default()
        };
        let creq = CanonicalRequest::from(&req, &signing_settings, date, None).unwrap();
        assert_eq!(&creq.content_sha256, "UNSIGNED-PAYLOAD");
        assert!(creq.to_string().ends_with("UNSIGNED-PAYLOAD"));
    }

    #[test]
    fn test_precomputed_payload() {
        let payload_hash = "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072";
        let req = test_request("get-vanilla-query-order-key-case");
        let req = SignableRequest::new(
            req.method(),
            req.uri(),
            req.headers(),
            SignableBody::Precomputed(String::from(payload_hash)),
        );
        let date = parse_date_time("20150830T123600Z").unwrap();
        let signing_settings = SigningSettings {
            payload_checksum_kind: PayloadChecksumKind::XAmzSha256,
            ..Default::default()
        };
        let creq = CanonicalRequest::from(&req, &signing_settings, date, None).unwrap();
        assert_eq!(&creq.content_sha256, payload_hash);
        assert!(creq.to_string().ends_with(payload_hash));
    }

    #[test]
    fn test_generate_scope() {
        let expected = "20150830/us-east-1/iam/aws4_request\n";
        let date = parse_date_time("20150830T123600Z").unwrap();
        let scope = Scope {
            date: date.date(),
            region: "us-east-1",
            service: "iam",
        };
        assert_eq!(format!("{}\n", scope.to_string()), expected);
    }

    #[test]
    fn test_string_to_sign() {
        let date = parse_date_time("20150830T123600Z").unwrap();
        let creq = test_canonical_request("get-vanilla-query-order-key-case");
        let expected_sts = test_sts("get-vanilla-query-order-key-case");
        let encoded = sha256_hex_string(creq.as_bytes());

        let actual = StringToSign::new(date, "us-east-1", "service", &encoded);
        assert_eq!(expected_sts, actual.to_string());
    }

    #[test]
    fn read_sts() {
        let sts = test_sts("get-vanilla-query-order-key-case");
        StringToSign::try_from(sts.as_ref()).unwrap();
    }

    #[test]
    fn test_digest_of_canonical_request() {
        let creq = test_canonical_request("get-vanilla-query-order-key-case");
        let expected = "816cd5b414d056048ba4f7c5386d6e0533120fb1fcfa93762cf0fc39e2cf19e0";
        let actual = sha256_hex_string(creq.as_bytes());
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_double_url_encode() {
        let req = test_request("double-url-encode");
        let req = SignableRequest::from_http(&req);
        let date = parse_date_time("20210511T154045Z").unwrap();
        let creq = CanonicalRequest::from(&req, &SigningSettings::default(), date, None).unwrap();

        let expected = test_canonical_request("double-url-encode");
        let actual = format!("{}", creq);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tilde_in_uri() {
        let req = http::Request::builder()
            .uri("https://s3.us-east-1.amazonaws.com/my-bucket?list-type=2&prefix=~objprefix&single&k=&unreserved=-_.~").body("").unwrap();
        let req = SignableRequest::from_http(&req);
        let date = parse_date_time("20210511T154045Z").unwrap();
        let creq = CanonicalRequest::from(&req, &SigningSettings::default(), date, None).unwrap();
        assert_eq!(
            Some("k=&list-type=2&prefix=~objprefix&single=&unreserved=-_.~"),
            creq.params.as_deref(),
        );
    }
}
