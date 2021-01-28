use std::borrow::Cow;
use std::time::SystemTime;
use std::error::Error;
use auth::Credentials;

#[derive(Eq, PartialEq, Clone, Copy)]
pub enum SigningAlgorithm {
    SigV4,
}

#[derive(Eq, PartialEq, Clone, Copy)]
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

impl OperationSigningConfig {
    pub fn default_config(service: &'static str) -> Self {
        OperationSigningConfig {
            algorithm: SigningAlgorithm::SigV4,
            signature_type: HttpSignatureType::HttpRequestHeaders,
            service: service.into(),
            double_uri_encode: false,
            normalize_uri_path: true,
            omit_session_token: false,
        }
    }
}

pub struct HttpSigningConfig {
    pub operation_config: OperationSigningConfig,
    pub request_config: RequestConfig,
}

#[derive(Clone, PartialEq, Eq)]
pub struct OperationSigningConfig {
    pub algorithm: SigningAlgorithm,
    pub signature_type: HttpSignatureType,
    pub service: Cow<'static, str>,

    pub double_uri_encode: bool,
    pub normalize_uri_path: bool,
    pub omit_session_token: bool,
}

pub struct RequestConfig {
    // the request config must enable recomputing the timestamp for retries, etc.
    pub request_ts: SystemTime,
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
    ) -> Result<(), SigningError>
        where
            B: AsRef<[u8]>,
    {
        let operation_config = &signing_config.operation_config;
        if operation_config.algorithm != SigningAlgorithm::SigV4
            || operation_config.double_uri_encode
            || !operation_config.normalize_uri_path
            || operation_config.omit_session_token
            || operation_config.signature_type != HttpSignatureType::HttpRequestHeaders
        {
            unimplemented!()
        }
        // TODO: update the aws_sigv4 API to avoid needing the clone the credentials
        let sigv4_creds = aws_sigv4::Credentials {
            access_key: credentials.access_key_id().to_string(),
            secret_key: credentials.secret_access_key().to_string(),
            security_token: credentials.session_token().map(|s|s.to_string()),
        };
        let date = signing_config.request_config.request_ts;
        for (key, value) in aws_sigv4::sign_core(
            request,
            &sigv4_creds,
            &signing_config.request_config.region,
            &signing_config.operation_config.service,
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
    #[test]
    fn hello() {}
}
