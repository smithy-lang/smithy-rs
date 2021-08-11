use chrono::{DateTime, Utc};
use http::header;
use serde::{Deserialize, Serialize};
use std::{iter, str};

pub const HMAC_256: &str = "AWS4-HMAC-SHA256";
pub const DATE_FORMAT: &str = "%Y%m%dT%H%M%SZ";
pub const X_AMZ_SECURITY_TOKEN: &str = "x-amz-security-token";
pub const X_AMZ_DATE: &str = "x-amz-date";
pub const X_AMZ_TARGET: &str = "x-amz-target";

pub mod sign;
pub mod types;

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

use crate::UriEncoding::Double;
use http::header::HeaderName;
use sign::{calculate_signature, encode_with_hex, generate_signing_key};
use std::time::SystemTime;
use types::{AsSigV4, CanonicalRequest, DateTimeExt, StringToSign};

pub fn sign<B>(
    req: &mut http::Request<B>,
    credential: &Credentials,
    region: &str,
    svc: &str,
) -> Result<(), Error>
where
    B: AsRef<[u8]>,
{
    for (header_name, header_value) in sign_core(
        &req,
        Config {
            access_key: &credential.access_key,
            secret_key: &credential.secret_key,
            security_token: credential.security_token.as_deref(),
            region,
            svc,
            date: SystemTime::now(),
            settings: Default::default(),
        },
    ) {
        req.headers_mut()
            .append(header_name.header_name(), header_value.parse()?);
    }

    Ok(())
}

/// SignatureKey is the key portion of the key-value pair of a generated SigV4 signature.
///
/// When signing with SigV4, the algorithm produces multiple components of a signature that MUST
/// be applied to a request.
pub enum SignatureKey {
    Authorization,
    AmzDate,
    AmzSecurityToken,
}

impl SignatureKey {
    pub fn header_name(&self) -> HeaderName {
        match self {
            SignatureKey::Authorization => header::AUTHORIZATION,
            SignatureKey::AmzDate => HeaderName::from_static(X_AMZ_DATE),
            SignatureKey::AmzSecurityToken => HeaderName::from_static(X_AMZ_SECURITY_TOKEN),
        }
    }
}

pub struct Config<'a> {
    pub access_key: &'a str,
    pub secret_key: &'a str,
    pub security_token: Option<&'a str>,

    pub region: &'a str,
    pub svc: &'a str,

    pub date: SystemTime,

    pub settings: SigningSettings,
}

#[derive(Debug, PartialEq)]
pub struct SigningSettings {
    /// We assume the URI will be encoded _once_ prior to transmission. Some services
    /// do not decode the path prior to checking the signature, requiring clients to actually
    /// _double-encode_ the URI in creating the canonical request in order to pass a signature check.
    pub uri_encoding: UriEncoding,
}

#[non_exhaustive]
#[derive(Debug, Eq, PartialEq)]
pub enum UriEncoding {
    /// Re-encode the resulting URL (eg. %30 becomes `%2530)
    Double,

    /// Take the resulting URL as-is
    Single,
}

impl Default for SigningSettings {
    fn default() -> Self {
        Self {
            uri_encoding: Double,
        }
    }
}

pub fn sign_core<'a, B>(
    req: &'a http::Request<B>,
    config: Config<'a>,
) -> impl Iterator<Item = (SignatureKey, String)>
where
    B: AsRef<[u8]>,
{
    let Config {
        access_key,
        secret_key,
        security_token,
        region,
        svc,
        date,
        settings,
    } = config;
    // Step 1: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-create-canonical-request.html.
    let creq = CanonicalRequest::from(req, &settings).unwrap();

    // Step 2: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-create-string-to-sign.html.
    let encoded_creq = &encode_with_hex(creq.fmt());
    let date = DateTime::<Utc>::from(date);
    let sts = StringToSign::new(date, region, svc, encoded_creq);

    // Step 3: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-calculate-signature.html
    let signing_key = generate_signing_key(secret_key, date.date(), region, svc);
    let signature = calculate_signature(signing_key, &sts.fmt().as_bytes());

    // Step 4: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-add-signature-to-request.html
    let authorization = build_authorization_header(access_key, creq, sts, &signature);
    let x_azn_date = date.fmt_aws();

    let mut tok = security_token.map(|it| it.to_string());
    iter::once((SignatureKey::Authorization, authorization))
        .chain(iter::once((SignatureKey::AmzDate, x_azn_date)))
        .chain(iter::from_fn(move || {
            tok.take().map(|tok| (SignatureKey::AmzSecurityToken, tok))
        }))
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Default, Clone)]
pub struct Credentials<'a> {
    #[serde(rename = "aws_access_key_id")]
    pub access_key: &'a str,
    #[serde(rename = "aws_secret_access_key")]
    pub secret_key: &'a str,
    #[serde(rename = "aws_session_token")]
    pub security_token: Option<&'a str>,
}

impl<'a> Credentials<'a> {
    pub fn new(access_key: &'a str, secret_key: &'a str, security_token: Option<&'a str>) -> Self {
        Self {
            access_key,
            secret_key,
            security_token,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        assert_req_eq, build_authorization_header, read,
        sign::{calculate_signature, encode_with_hex, generate_signing_key},
        types::{AsSigV4, CanonicalRequest, DateExt, DateTimeExt, Scope, StringToSign},
        Error, SigningSettings, DATE_FORMAT,
    };
    use chrono::{Date, DateTime, NaiveDateTime, Utc};
    use http::{Method, Request, Uri, Version};
    use pretty_assertions::assert_eq;
    use std::{convert::TryFrom, fs, str::FromStr};

    #[test]
    fn read_request() -> Result<(), Error> {
        //file-name.req—the web request to be signed.
        //file-name.creq—the resulting canonical request.
        //file-name.sts—the resulting string to sign.
        //file-name.authz—the Authorization header.
        //file-name.sreq— the signed request.

        // Step 1: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-create-canonical-request.html.
        // let s = read!(req: "get-vanilla-query-order-key-case")?;
        let s = std::fs::read_to_string("./aws-sig-v4-test-suite/get-vanilla-query-order-key-case/get-vanilla-query-order-key-case.req").unwrap();
        let mut req = parse_request(s.as_bytes())?;
        let creq = CanonicalRequest::from(&mut req, &SigningSettings::default())?;

        let actual = format!("{}", creq);
        let expected = read!(creq: "get-vanilla-query-order-key-case");
        assert_eq!(actual, expected);

        // Step 2: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-create-string-to-sign.html.
        let date = NaiveDateTime::parse_from_str("20150830T123600Z", DATE_FORMAT).unwrap();
        let date = DateTime::<Utc>::from_utc(date, Utc);
        let encoded_creq = &encode_with_hex(creq.fmt());
        let sts = StringToSign::new(date, "us-east-1", "service", encoded_creq);

        // Step 3: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-calculate-signature.html
        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";

        let signing_key = generate_signing_key(secret, date.date(), "us-east-1", "service");
        let signature = calculate_signature(signing_key, &sts.fmt().as_bytes());
        let access = "AKIDEXAMPLE";

        // step 4: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-add-signature-to-request.html
        let authorization = build_authorization_header(access, creq, sts, &signature);
        let x_azn_date = date.fmt_aws();

        let s = read!(req: "get-vanilla-query-order-key-case");
        let mut req = parse_request(s.as_bytes())?;

        let headers = req.headers_mut();
        headers.insert("authorization", authorization.parse()?);
        headers.insert("X-Amz-Date", x_azn_date.parse()?);
        let expected = read!(sreq: "get-vanilla-query-order-key-case");
        let expected = parse_request(expected.as_bytes())?;
        assert_req_eq!(expected, req);

        Ok(())
    }

    #[test]
    fn test_build_authorization_header() -> Result<(), Error> {
        let s = read!(req: "get-vanilla-query-order-key-case");
        let mut req = parse_request(s.as_bytes())?;
        let creq = CanonicalRequest::from(&mut req, &SigningSettings::default())?;

        let date = NaiveDateTime::parse_from_str("20150830T123600Z", DATE_FORMAT).unwrap();
        let date = DateTime::<Utc>::from_utc(date, Utc);
        let encoded_creq = &encode_with_hex(creq.fmt());
        let sts = StringToSign::new(date, "us-east-1", "service", encoded_creq);

        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let signing_key = generate_signing_key(secret, date.date(), "us-east-1", "service");
        let signature = calculate_signature(signing_key, &sts.fmt().as_bytes());
        let expected_header = read!(authz: "get-vanilla-query-order-key-case");
        let header = build_authorization_header("AKIDEXAMPLE", creq, sts, &signature);
        assert_eq!(expected_header, header);

        Ok(())
    }

    #[test]
    fn test_generate_scope() -> Result<(), Error> {
        let expected = "20150830/us-east-1/iam/aws4_request\n";
        let date = DateTime::parse_aws("20150830T123600Z")?;
        let scope = Scope {
            date: date.date(),
            region: "us-east-1",
            service: "iam",
        };
        assert_eq!(format!("{}\n", scope.fmt()), expected);

        Ok(())
    }

    #[test]
    fn test_parse() -> Result<(), Error> {
        let buf = read!(req: "post-header-key-case");
        parse_request(buf.as_bytes())?;
        Ok(())
    }

    #[test]
    fn test_read_query_params() -> Result<(), Error> {
        let buf = read!(req: "get-vanilla-query-order-key-case");
        parse_request(buf.as_bytes()).unwrap();
        Ok(())
    }

    #[test]
    fn test_parse_headers() {
        let buf = b"Host:example.amazonaws.com\nX-Amz-Date:20150830T123600Z\n\nblah blah";
        let mut headers = [httparse::EMPTY_HEADER; 4];
        assert_eq!(
            httparse::parse_headers(buf, &mut headers),
            Ok(httparse::Status::Complete((
                56,
                &[
                    httparse::Header {
                        name: "Host",
                        value: b"example.amazonaws.com",
                    },
                    httparse::Header {
                        name: "X-Amz-Date",
                        value: b"20150830T123600Z",
                    }
                ][..]
            )))
        );
    }

    #[test]
    fn sign_payload_empty_string() {
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let actual = encode_with_hex(String::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn datetime_format() -> Result<(), Error> {
        let date = DateTime::parse_aws("20150830T123600Z")?;
        let expected = "20150830T123600Z";
        assert_eq!(expected, date.fmt_aws());

        Ok(())
    }

    #[test]
    fn date_format() -> Result<(), Error> {
        let date = Date::parse_aws("20150830")?;
        let expected = "20150830";
        assert_eq!(expected, date.fmt_aws());

        Ok(())
    }

    #[test]
    fn test_string_to_sign() -> Result<(), Error> {
        let date = DateTime::parse_aws("20150830T123600Z")?;
        let creq = read!(creq: "get-vanilla-query-order-key-case");
        let expected_sts = read!(sts: "get-vanilla-query-order-key-case");
        let encoded = encode_with_hex(creq);

        let actual = StringToSign::new(date, "us-east-1", "service", &encoded);
        assert_eq!(expected_sts, actual.fmt());

        Ok(())
    }

    #[test]
    fn test_signature_calculation() -> Result<(), Error> {
        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let creq = fs::read_to_string("./aws-sig-v4-test-suite/iam.creq")?.replace("\r\n", "\n");
        let date = DateTime::parse_aws("20150830T123600Z")?;

        let derived_key = generate_signing_key(secret, date.date(), "us-east-1", "iam");
        let signature = calculate_signature(derived_key, creq.as_bytes());

        let expected = "5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7";
        assert_eq!(expected, &signature);

        Ok(())
    }

    #[test]
    fn parse_signed_request() -> Result<(), Error> {
        let req = read!(sreq: "post-header-key-case");
        let _: Request<_> = parse_request(req.as_bytes())?;
        Ok(())
    }

    #[test]
    fn read_sts() -> Result<(), Error> {
        let sts = read!(sts: "get-vanilla-query-order-key-case");
        let _ = StringToSign::try_from(sts.as_ref())?;
        Ok(())
    }

    #[test]
    fn test_digest_of_canonical_request() -> Result<(), Error> {
        let creq = read!(creq: "get-vanilla-query-order-key-case");
        let actual = encode_with_hex(creq);
        let expected = "816cd5b414d056048ba4f7c5386d6e0533120fb1fcfa93762cf0fc39e2cf19e0";

        assert_eq!(expected, actual);
        Ok(())
    }

    #[test]
    fn test_double_url_encode() -> Result<(), Error> {
        let s = read!(req: "double-url-encode");
        let req = parse_request(s.as_bytes())?;
        println!("{:?}", req);
        let creq = CanonicalRequest::from(&req, &SigningSettings::default())?;

        let actual = format!("{}", creq);
        let expected = read!(creq: "double-url-encode");
        println!("{}", actual);
        assert_eq!(actual, expected);
        Ok(())
    }

    fn parse_request(s: &[u8]) -> Result<Request<bytes::Bytes>, Error> {
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = httparse::Request::new(&mut headers);
        let _ = req.parse(s).unwrap();

        let version = match req.version.unwrap() {
            1 => Version::HTTP_11,
            _ => unimplemented!(),
        };

        let method = match req.method.unwrap() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            _ => unimplemented!(),
        };

        let builder = Request::builder();
        let builder = builder.version(version);
        let mut builder = builder.method(method);
        if let Some(path) = req.path {
            builder = builder.uri(Uri::from_str(path)?);
        }
        for header in req.headers {
            let name = header.name.to_lowercase();
            if !name.is_empty() {
                builder = builder.header(&name, header.value);
            }
        }

        let req = builder.body(bytes::Bytes::new())?;
        Ok(req)
    }
}

// add signature to authorization header
// Authorization: algorithm Credential=access key ID/credential scope, SignedHeaders=SignedHeaders, Signature=signature
fn build_authorization_header(
    access_key: &str,
    creq: CanonicalRequest,
    sts: StringToSign,
    signature: &str,
) -> String {
    format!(
        "{} Credential={}/{}, SignedHeaders={}, Signature={}",
        HMAC_256,
        access_key,
        sts.scope.fmt(),
        creq.signed_headers,
        signature
    )
}

#[macro_export]
macro_rules! assert_req_eq {
    ($a:tt, $b:tt) => {
        assert_eq!(format!("{:?}", $a), format!("{:?}", $b))
    };
}

#[macro_export]
macro_rules! read {
    (req: $case:tt) => {
        fs::read_to_string(format!("./aws-sig-v4-test-suite/{}/{}.req", $case, $case))?
            // this replacement is necessary for tests to pass on Windows, as reading the
            // sigv4 snapshots from the file system results in CRLF line endings being inserted.
            .replace("\r\n", "\n")
    };

    (creq: $case:tt) => {
        fs::read_to_string(format!("./aws-sig-v4-test-suite/{}/{}.creq", $case, $case))?
            .replace("\r\n", "\n")
    };

    (sreq: $case:tt) => {
        fs::read_to_string(format!("./aws-sig-v4-test-suite/{}/{}.sreq", $case, $case))?
            .replace("\r\n", "\n")
    };

    (sts: $case:tt) => {
        fs::read_to_string(format!("./aws-sig-v4-test-suite/{}/{}.sts", $case, $case))?
            .replace("\r\n", "\n")
    };

    (authz: $case:tt) => {
        fs::read_to_string(format!("./aws-sig-v4-test-suite/{}/{}.authz", $case, $case))?
            .replace("\r\n", "\n")
    };
}
