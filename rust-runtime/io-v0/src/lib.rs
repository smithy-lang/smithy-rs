use aws_sigv4::{sign, Credentials, Region, RequestExt, SignedService};
use http_body::Body;
use hyper::body::Buf;
use hyper::client::HttpConnector;
use hyper::http::request;
use hyper::{Client as HyperClient, Request, Response, Uri};
pub use http;

/// macro to execute an AWS request, currently required because no traits exist
/// to specify the required methods.
///
/// # Example
/// ```rust
/// let client = Client::local("dynamodb");
/// let clear_tables = operation::DeleteTable::builder().table_name("my_table").build();
/// let cleared = make_request!(hyper_client, clear_tables);
/// ```
#[macro_export]
macro_rules! dispatch {
    ($client:expr, $input:expr) => {{
        use $crate::http;
        let inp = $input;
        let request = inp.build_http_request();
        let request = $crate::prepare_request(&$client, request);
        let response = $client.http_client.request(request).await;
        match response {
            Ok(response) => match $crate::prepare_response(response).await {
                Err((parts, body_err)) => $crate::ApiResponse {
                    raw: $crate::Raw::ReadFailure(http::Response::from_parts(parts, ()), body_err),
                    parsed: None,
                },
                Ok(resp) => {
                    let parsed = Some(inp.parse_response(&resp));
                    $crate::ApiResponse {
                        raw: $crate::Raw::Response(resp).convert_to_str(),
                        parsed,
                    }
                }
            },
            Err(e) => $crate::ApiResponse {
                raw: $crate::Raw::DispatchFailure(e),
                parsed: None,
            },
        }
    }};
}

#[derive(Debug)]
pub enum Raw {
    DispatchFailure(hyper::Error),
    ReadFailure(hyper::Response<()>, hyper::Error),
    Response(hyper::Response<Vec<u8>>),
    PrettyResponse(hyper::Response<String>)
}

impl Raw {
    /// Attempt to convert the raw response into a string, otherwise, leaves it as raw UTF-8
    pub fn convert_to_str(self) -> Self {
        match self {
            Raw::Response(resp) => {
                let (parts, body) = resp.into_parts();
                match String::from_utf8(body) {
                    Ok(s) => Raw::PrettyResponse(Response::from_parts(parts, s)),
                    Err(e) => Raw::Response(Response::from_parts(parts, e.into_bytes()))
                }
            },
            other => other
        }
    }
}

#[derive(Debug)]
pub struct ApiResponse<P: std::fmt::Debug> {
    pub raw: Raw,
    pub parsed: Option<P>,
}

impl<P: std::fmt::Debug> ApiResponse<P> {
    pub fn parsed(&self) -> Result<&P, &Raw> {
        match &self.parsed {
            Some(p) => Ok(&p),
            None => Err(&self.raw)
        }
    }
}

pub struct Client<C> {
    pub http_client: C,
    pub credentials: Credentials,
    pub config: Config,
}

impl Client<hyper::Client<HttpConnector, hyper::Body>> {
    pub fn local(service: &'static str) -> Self {
        let hyper_client: HyperClient<HttpConnector, hyper::Body> =
            hyper::Client::builder().build_http();
        Client {
            http_client: hyper_client,
            credentials: Credentials::new("unused", "unused", None),
            config: Config {
                authority: "localhost:8000".to_string(),
                scheme: "http".to_string(),
                region: "us-east-1",
                service,
            },
        }
    }
}

pub struct Config {
    authority: String,
    scheme: String,
    region: &'static str,
    service: &'static str,
}

pub fn prepare_request<C>(
    client: &Client<C>,
    mut request: request::Request<Vec<u8>>,
) -> Request<hyper::Body> {
    // TODO: this should be handled by the config
    let uri = Uri::builder()
        .authority(client.config.authority.as_str())
        .scheme(client.config.scheme.as_str())
        .path_and_query(request.uri().path_and_query().unwrap().clone())
        .build()
        .expect("valid uri");

    (*request.uri_mut()) = uri;

    request.set_region(Region::new(&client.config.region));
    request.set_service(SignedService::new(&client.config.service));
    sign(&mut request, &client.credentials).unwrap();
    request.map(|body| body.into())
}

pub async fn prepare_response(
    response: Response<hyper::Body>,
) -> Result<
    Response<Vec<u8>>,
    (
        http::response::Parts,
        <hyper::Body as http_body::Body>::Error,
    ),
> {
    let (parts, body) = response.into_parts();
    match read_body(body).await {
        Ok(data) => Ok(Response::from_parts(parts, data)),
        Err(e) => Err((parts, e)),
    }
}

async fn read_body<B: http_body::Body>(body: B) -> Result<Vec<u8>, B::Error> {
    let mut output = Vec::new();
    pin_utils::pin_mut!(body);
    while let Some(buf) = body.data().await {
        let buf = buf?;
        if buf.has_remaining() {
            output.extend_from_slice(buf.bytes())
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use crate::Client;
    use hyper::http;
    use std::error::Error;

    struct TestOperation;

    impl TestOperation {
        pub fn build_http_request(&self) -> http::Request<Vec<u8>> {
            http::Request::new(vec![])
        }

        pub fn parse_response(&self, reponse: &http::Response<Vec<u8>>) -> u32 {
            5
        }
    }

    #[tokio::test]
    async fn make_test_request() -> Result<(), Box<dyn Error>> {
        let client = Client::local("dynamodb");
        let response = dispatch!(client, TestOperation);
        Ok(())
    }
}
