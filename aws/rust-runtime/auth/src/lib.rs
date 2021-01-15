use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::{Instant, SystemTime};
use std::borrow::Cow;

/// AWS SDK Credentials
///
/// An opaque struct representing credentials that may be used in an AWS SDK, modeled on
/// the [CRT credentials implementation](https://github.com/awslabs/aws-c-auth/blob/main/source/credentials.c).
#[derive(Clone)]
pub struct Credentials {
    // TODO: consider if these fields should be Arc'd; Credentials are cloned when
    // retrieved from a credentials provider.
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,

    /// Credential Expiry
    ///
    /// A timepoint at which the credentials should no longer
    /// be used because they have expired. The primary purpose of this value is to allow
    /// credentials to communicate to the caching provider when they need to be refreshed.
    ///
    /// If these credentials never expire, this value will be set to `None`
    ///
    /// TODO: consider if `Instant` is the best representation—other options:
    /// - SystemTime, we don't need monotonicity for this
    /// - u64
    expiration: Option<Instant>,
}

impl Credentials {
    /// Create a Credentials struct from static credentials
    pub fn from_static(
        access_key_id: impl ToString,
        secret_access_key: impl ToString,
    ) -> Self {
        Credentials {
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            session_token: None,
            expiration: None,
        }
    }
}

/// An error retrieving credentials from a credentials provider
///
/// TODO: consider possible explicit variants
#[derive(Debug)]
pub struct CredentialsProviderError {
    cause: Box<dyn Error + Send>,
}

impl Display for CredentialsProviderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.cause.fmt(f)
    }
}

impl Error for CredentialsProviderError {}

// TODO
type CredentialsError = Box<dyn Error>;

/// A credentials provider
///
/// Credentials providers may be sync or async, they may cache credentials, etc.
///
/// WARNING: This interface is unstable pending async traits
pub trait ProvideCredentials: Send + Sync {
    fn credentials(&self) -> Result<Credentials, CredentialsError>;
}

pub fn default_provider() -> impl ProvideCredentials {
    // todo: this should be a chain, maybe CRT?
    // Determine what the minimum support is
    Credentials::from_static("todo", "todo")
}

impl ProvideCredentials for Credentials {
    fn credentials(&self) -> Result<Credentials, CredentialsError> {
        Ok(self.clone())
    }
}

#[derive(Eq, PartialEq)]
pub enum SigningAlgorithm {
    SigV4,
}

#[derive(Eq, PartialEq)]
pub enum HttpSignatureType {
    /// A signature for a full http request should be computed, with header updates applied to the signing result.
    HttpRequestHeaders,
    /// A signature for a full http request should be computed, with query param updates applied to the signing result.
    HttpRequestQueryParams,
}

pub enum SigningConfig {
    Http(HttpSigningConfig),
    // Http Chunked Body
    // Event Stream
}

pub struct HttpSigningConfig {
    pub algorithm: SigningAlgorithm,
    pub signature_type: HttpSignatureType,
    pub service_config: ServiceConfig,
    pub request_config: RequestConfig,

    pub double_uri_encode: bool,
    pub normalize_uri_path: bool,
    pub omit_session_token: bool,
}

pub struct RequestConfig {
    // the request config must enable recomputing the timestamp for retries, etc.
    pub request_ts: fn() -> SystemTime,
}

pub struct ServiceConfig {
    pub service: Cow<'static, str>,
    pub region: Cow<'static, str>,
}

type SigningError = Box<dyn Error + Send + Sync + 'static>;

#[derive(Clone)]
pub struct HttpSigner {}

impl HttpSigner {
    /// Sign an HTTP request
    ///
    /// You may need to modify the body of your HTTP request to be signable.
    ///
    /// NOTE: This design may change, a modified design that returns a SigningResult may be
    /// used instead, this design allows for current compatibility with `aws-sigv4`
    pub fn sign<B>(
        &self,
        signing_config: &HttpSigningConfig,
        credentials: &Credentials,
        request: &mut http::Request<B>,
        payload: impl AsRef<[u8]>,
    ) -> Result<(), SigningError> {
        if signing_config.algorithm != SigningAlgorithm::SigV4
            || signing_config.double_uri_encode
            || !signing_config.normalize_uri_path
            || signing_config.omit_session_token
            || signing_config.signature_type != HttpSignatureType::HttpRequestHeaders
        {
            unimplemented!()
        }
        // TODO: update the aws_sigv4 API to avoid needing the clone the credentials
        let sigv4_creds = aws_sigv4::Credentials {
            access_key: credentials.access_key_id.clone(),
            secret_key: credentials.secret_access_key.clone(),
            security_token: credentials.session_token.clone(),
        };
        let date = (signing_config.request_config.request_ts)();
        for (key, value) in aws_sigv4::sign_core(
            request,
            payload,
            &sigv4_creds,
            &signing_config.service_config.region,
            &signing_config.service_config.service,
            date,
        ) {
            request
                .headers_mut()
                .append(key.header_name(), value.parse()?);
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::{
        Credentials, CredentialsProviderError, HttpSignatureType, HttpSigner, HttpSigningConfig,
        ProvideCredentials, RequestConfig, ServiceConfig, SigningAlgorithm,
    };
    use std::error::Error;
    use std::time::SystemTime;

    struct DynamoConfig {
        signer: HttpSigner,
        credentials_provider: Box<dyn ProvideCredentials>,
        service: &'static str,
        region: &'static str,
        time_source: fn() -> SystemTime,
    }

    impl DynamoConfig {
        pub fn from_env() -> Self {
            let stub_creds = Credentials::from_static("asdf", "asef");
            DynamoConfig {
                signer: HttpSigner {},
                credentials_provider: Box::new(stub_creds),
                service: "dynamodb",
                region: "us-east-1",
                time_source: || SystemTime::now(),
            }
        }
    }

    struct ListTablesOperationInput {
        field: u32,
    }

    struct ListTablesOperation {
        input: ListTablesOperationInput,
    }

    use std::borrow::Cow;

    impl ListTablesOperation {
        pub fn raw_request(
            &self,
            _config: &DynamoConfig,
        ) -> Result<http::Request<Vec<u8>>, Box<dyn Error + Send + Sync>> {
            let base = http::Request::new(vec![self.input.field as u8]);
            Ok(base)
        }

        pub async fn build_request(&self, config: &DynamoConfig) -> http::Request<Vec<u8>> {
            let mut raw_request = self.raw_request(&config).unwrap();
            Self::finalize(&config, &mut raw_request).await;
            raw_request
        }
        pub async fn finalize(config: &DynamoConfig, mut request: &mut http::Request<Vec<u8>>) {
            let service_config = ServiceConfig {
                service: Cow::Borrowed(config.service),
                region: Cow::Borrowed(config.region),
            };
            let signing_config = ListTablesOperation::signing_config(
                RequestConfig {
                    request_ts: config.time_source,
                },
                service_config,
            );
            let credentials = config.credentials_provider.credentials().unwrap();
            config
                .signer
                .sign(&signing_config, &credentials, &mut request, &[1, 2, 3])
                .expect("signing failed");
            // Ok(base)
        }

        fn signing_config(
            request_config: RequestConfig,
            service_config: ServiceConfig,
        ) -> HttpSigningConfig {
            // Different operations can use different signing configurations
            HttpSigningConfig {
                algorithm: SigningAlgorithm::SigV4,
                signature_type: HttpSignatureType::HttpRequestHeaders,
                service_config,
                request_config,
                double_uri_encode: false,
                normalize_uri_path: true,
                omit_session_token: false,
            }
        }
    }

    impl ListTablesOperationInput {
        fn build(
            self,
            _config: &DynamoConfig,
        ) -> Result<ListTablesOperation, CredentialsProviderError> {
            Ok(ListTablesOperation { input: self })
        }
    }

    #[tokio::test]
    async fn sign_operation() {
        let inp = ListTablesOperationInput { field: 123 };
        let dynamo = DynamoConfig::from_env();
        // Various problems, eg. credentials provider error
        let operation = inp.build(&dynamo).expect("failed to get creds");
        // Eg. signing error, expired credentials, etc.
        let base_request = operation.build_request(&dynamo).await;
        assert_eq!(
            base_request
                .headers()
                .keys()
                .map(|k| k.as_str())
                .collect::<Vec<_>>(),
            vec!["authorization", "x-amz-date"]
        );
    }
}
