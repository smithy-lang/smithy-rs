use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc;
use std::thread;

use aws_smithy_http_server::body::{Body as ServerBody, BoxBody};
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpClient,
    SharedHttpConnector,
};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::body::SdkBody;
use bytes::Bytes;
use http_body_util::BodyExt;
use tower::Service;

pub type CaseFuture = Pin<Box<dyn Future<Output = ()> + 'static>>;

#[derive(Clone, Copy)]
pub struct BenchmarkCase {
    pub name: &'static str,
    make_runner: fn(&'static str) -> CaseRunner,
}

impl BenchmarkCase {
    pub fn runner(self) -> CaseRunner {
        (self.make_runner)(self.name)
    }

    pub fn run(self) -> CaseFuture {
        let runner = self.runner();
        Box::pin(async move {
            runner.run().await;
        })
    }
}

pub struct CaseRunner {
    pub name: &'static str,
    run: Box<dyn Fn() -> CaseFuture>,
}

impl CaseRunner {
    pub fn run(&self) -> CaseFuture {
        (self.run)()
    }
}

pub fn case_by_name(name: &str) -> Option<BenchmarkCase> {
    CASES.iter().copied().find(|case| case.name == name)
}

pub fn case_names() -> impl Iterator<Item = &'static str> {
    CASES.iter().map(|case| case.name)
}

#[derive(Clone)]
struct InProcessHttpClient {
    sender: mpsc::Sender<Work>,
    request_overrides: RequestOverrides,
}

#[derive(Clone, Copy)]
struct RequestOverrides {
    extra_headers: &'static [(&'static str, &'static str)],
    method: Option<&'static str>,
    uri: Option<&'static str>,
}

impl RequestOverrides {
    const NONE: Self = Self {
        extra_headers: &[],
        method: None,
        uri: None,
    };
}

struct Work {
    request: http::Request<ServerBody>,
    response: mpsc::Sender<Result<http::Response<Bytes>, String>>,
}

impl fmt::Debug for InProcessHttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InProcessHttpClient").finish()
    }
}

fn connector_error(error: impl Into<BoxError>) -> ConnectorError {
    ConnectorError::other(error.into(), None)
}

impl HttpConnector for InProcessHttpClient {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let sender = self.sender.clone();
        let request_overrides = self.request_overrides;
        HttpConnectorFuture::new(async move {
            let request = request.try_into_http1x().map_err(connector_error)?;
            let (mut parts, body) = request.into_parts();
            if let Some(method) = request_overrides.method {
                parts.method = method.parse().map_err(connector_error)?;
            }
            if let Some(uri) = request_overrides.uri {
                parts.uri = uri.parse().map_err(connector_error)?;
            }
            let mut request = http::Request::from_parts(parts, ServerBody::new(body));
            for (name, value) in request_overrides.extra_headers {
                request.headers_mut().insert(
                    http::header::HeaderName::from_static(name),
                    http::header::HeaderValue::from_static(value),
                );
            }

            let (response_tx, response_rx) = mpsc::channel();
            sender
                .send(Work {
                    request,
                    response: response_tx,
                })
                .map_err(connector_error)?;

            let response = tokio::task::spawn_blocking(move || response_rx.recv())
                .await
                .map_err(connector_error)?
                .map_err(connector_error)?
                .map_err(connector_error)?;
            let (parts, body) = response.into_parts();
            let response = http::Response::from_parts(parts, SdkBody::from(body));
            HttpResponse::try_from(response).map_err(connector_error)
        })
    }
}

impl HttpClient for InProcessHttpClient {
    fn http_connector(
        &self,
        _settings: &HttpConnectorSettings,
        _components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        SharedHttpConnector::new(self.clone())
    }
}

fn http_client_with_overrides<S>(
    service: S,
    request_overrides: RequestOverrides,
) -> SharedHttpClient
where
    S: Service<http::Request<ServerBody>, Response = http::Response<BoxBody>, Error = Infallible>
        + Send
        + 'static,
{
    let (sender, receiver) = mpsc::channel::<Work>();
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        let mut service = service;
        while let Ok(work) = receiver.recv() {
            let result = runtime.block_on(async {
                let response = service
                    .call(work.request)
                    .await
                    .expect("server is infallible");
                let (parts, body) = response.into_parts();
                let body = body
                    .collect()
                    .await
                    .map_err(|err| err.to_string())?
                    .to_bytes();
                Ok(http::Response::from_parts(parts, body))
            });
            let _ = work.response.send(result);
        }
    });

    SharedHttpClient::new(InProcessHttpClient {
        sender,
        request_overrides,
    })
}

fn retry_config() -> aws_smithy_types::retry::RetryConfig {
    aws_smithy_types::retry::RetryConfig::disabled()
}

fn dynamic_service(
) -> impl Service<http::Request<ServerBody>, Response = http::Response<BoxBody>, Error = Infallible>
       + Send
       + 'static {
    async fn get_server_statistics(
        _input: dynamic_server::input::GetServerStatisticsInput,
    ) -> dynamic_server::output::GetServerStatisticsOutput {
        dynamic_server::output::GetServerStatisticsOutput { calls_count: 1 }
    }

    let config = dynamic_server::PokemonServiceConfig::builder().build();
    dynamic_server::PokemonService::builder(config)
        .get_server_statistics(get_server_statistics)
        .build_unchecked()
}

fn static_service(
) -> impl Service<http::Request<ServerBody>, Response = http::Response<BoxBody>, Error = Infallible>
       + Send
       + 'static {
    async fn get_server_statistics(
        _input: static_server::input::GetServerStatisticsInput,
    ) -> static_server::output::GetServerStatisticsOutput {
        static_server::output::GetServerStatisticsOutput { calls_count: 1 }
    }

    let config = static_server::PokemonServiceConfig::builder().build();
    static_server::PokemonService::builder(config)
        .get_server_statistics(get_server_statistics)
        .build_unchecked()
}

fn legacy_rest_json1_service(
) -> impl Service<http::Request<ServerBody>, Response = http::Response<BoxBody>, Error = Infallible>
       + Send
       + 'static {
    async fn get_server_statistics(
        _input: legacy_rest_json1_server::input::GetServerStatisticsInput,
    ) -> legacy_rest_json1_server::output::GetServerStatisticsOutput {
        legacy_rest_json1_server::output::GetServerStatisticsOutput { calls_count: 1 }
    }

    let config = legacy_rest_json1_server::PokemonServiceConfig::builder().build();
    legacy_rest_json1_server::PokemonService::builder(config)
        .get_server_statistics(get_server_statistics)
        .build_unchecked()
}

fn legacy_rest_xml_service(
) -> impl Service<http::Request<ServerBody>, Response = http::Response<BoxBody>, Error = Infallible>
       + Send
       + 'static {
    async fn get_server_statistics(
        _input: legacy_rest_xml_server::input::GetServerStatisticsInput,
    ) -> legacy_rest_xml_server::output::GetServerStatisticsOutput {
        legacy_rest_xml_server::output::GetServerStatisticsOutput { calls_count: 1 }
    }

    let config = legacy_rest_xml_server::PokemonServiceConfig::builder().build();
    legacy_rest_xml_server::PokemonService::builder(config)
        .get_server_statistics(get_server_statistics)
        .build_unchecked()
}

fn legacy_aws_json_10_service(
) -> impl Service<http::Request<ServerBody>, Response = http::Response<BoxBody>, Error = Infallible>
       + Send
       + 'static {
    async fn get_server_statistics(
        _input: legacy_aws_json_10_server::input::GetServerStatisticsInput,
    ) -> legacy_aws_json_10_server::output::GetServerStatisticsOutput {
        legacy_aws_json_10_server::output::GetServerStatisticsOutput { calls_count: 1 }
    }

    let config = legacy_aws_json_10_server::PokemonServiceConfig::builder().build();
    legacy_aws_json_10_server::PokemonService::builder(config)
        .get_server_statistics(get_server_statistics)
        .build_unchecked()
}

fn legacy_aws_json_11_service(
) -> impl Service<http::Request<ServerBody>, Response = http::Response<BoxBody>, Error = Infallible>
       + Send
       + 'static {
    async fn get_server_statistics(
        _input: legacy_aws_json_11_server::input::GetServerStatisticsInput,
    ) -> legacy_aws_json_11_server::output::GetServerStatisticsOutput {
        legacy_aws_json_11_server::output::GetServerStatisticsOutput { calls_count: 1 }
    }

    let config = legacy_aws_json_11_server::PokemonServiceConfig::builder().build();
    legacy_aws_json_11_server::PokemonService::builder(config)
        .get_server_statistics(get_server_statistics)
        .build_unchecked()
}

fn legacy_rpcv2_cbor_service(
) -> impl Service<http::Request<ServerBody>, Response = http::Response<BoxBody>, Error = Infallible>
       + Send
       + 'static {
    async fn get_server_statistics(
        _input: legacy_rpcv2_cbor_server::input::GetServerStatisticsInput,
    ) -> legacy_rpcv2_cbor_server::output::GetServerStatisticsOutput {
        legacy_rpcv2_cbor_server::output::GetServerStatisticsOutput { calls_count: 1 }
    }

    let config = legacy_rpcv2_cbor_server::PokemonServiceConfig::builder().build();
    legacy_rpcv2_cbor_server::PokemonService::builder(config)
        .get_server_statistics(get_server_statistics)
        .build_unchecked()
}

macro_rules! client_case {
    ($name:ident, $runner:ident, $client:ident, $service:expr) => {
        client_case!($name, $runner, $client, $service, RequestOverrides::NONE);
    };
    ($name:ident, $runner:ident, $client:ident, $service:expr, $request_overrides:expr) => {
        pub async fn $name() {
            $runner(stringify!($name)).run().await;
        }

        fn $runner(name: &'static str) -> CaseRunner {
            let config = $client::Config::builder()
                .behavior_version($client::config::BehaviorVersion::latest())
                .endpoint_url("http://localhost")
                .retry_config(retry_config())
                .http_client(http_client_with_overrides($service, $request_overrides))
                .build();
            let client = $client::Client::from_conf(config);

            CaseRunner {
                name,
                run: Box::new(move || {
                    let client = client.clone();
                    Box::pin(async move {
                        let output = client
                            .get_server_statistics()
                            .send()
                            .await
                            .expect("operation should succeed");
                        assert_eq!(output.calls_count(), 1);
                    })
                }),
            }
        }
    };
}

client_case!(
    rest_json1_legacy,
    rest_json1_legacy_runner,
    rest_json1_client,
    legacy_rest_json1_service()
);
client_case!(
    rest_json1_dynamic,
    rest_json1_dynamic_runner,
    rest_json1_client,
    dynamic_service()
);
client_case!(
    rest_json1_static,
    rest_json1_static_runner,
    rest_json1_client,
    static_service()
);

client_case!(
    rest_xml_legacy,
    rest_xml_legacy_runner,
    rest_xml_client,
    legacy_rest_xml_service()
);
client_case!(
    rest_xml_dynamic,
    rest_xml_dynamic_runner,
    rest_xml_client,
    dynamic_service(),
    RequestOverrides {
        extra_headers: &[("accept", "application/xml")],
        ..RequestOverrides::NONE
    }
);
client_case!(
    rest_xml_static,
    rest_xml_static_runner,
    rest_xml_client,
    static_service(),
    RequestOverrides {
        extra_headers: &[("accept", "application/xml")],
        ..RequestOverrides::NONE
    }
);

client_case!(
    aws_json_10_legacy,
    aws_json_10_legacy_runner,
    aws_json_10_client,
    legacy_aws_json_10_service(),
    RequestOverrides {
        extra_headers: &[
            ("x-amz-target", "PokemonService.GetServerStatistics"),
            ("content-type", "application/x-amz-json-1.0"),
            ("accept", "application/x-amz-json-1.0"),
        ],
        method: Some("POST"),
        uri: Some("/"),
    }
);
client_case!(
    aws_json_10_dynamic,
    aws_json_10_dynamic_runner,
    aws_json_10_client,
    dynamic_service(),
    RequestOverrides {
        extra_headers: &[
            ("x-amz-target", "PokemonService.GetServerStatistics"),
            ("content-type", "application/x-amz-json-1.0"),
            ("accept", "application/x-amz-json-1.0"),
        ],
        method: Some("POST"),
        uri: Some("/"),
    }
);
client_case!(
    aws_json_10_static,
    aws_json_10_static_runner,
    aws_json_10_client,
    static_service(),
    RequestOverrides {
        extra_headers: &[
            ("x-amz-target", "PokemonService.GetServerStatistics"),
            ("content-type", "application/x-amz-json-1.0"),
            ("accept", "application/x-amz-json-1.0"),
        ],
        method: Some("POST"),
        uri: Some("/"),
    }
);

client_case!(
    aws_json_11_legacy,
    aws_json_11_legacy_runner,
    aws_json_11_client,
    legacy_aws_json_11_service(),
    RequestOverrides {
        extra_headers: &[
            ("x-amz-target", "PokemonService.GetServerStatistics"),
            ("content-type", "application/x-amz-json-1.1"),
            ("accept", "application/x-amz-json-1.1"),
        ],
        method: Some("POST"),
        uri: Some("/"),
    }
);
client_case!(
    aws_json_11_dynamic,
    aws_json_11_dynamic_runner,
    aws_json_11_client,
    dynamic_service(),
    RequestOverrides {
        extra_headers: &[
            ("x-amz-target", "PokemonService.GetServerStatistics"),
            ("content-type", "application/x-amz-json-1.1"),
            ("accept", "application/x-amz-json-1.1"),
        ],
        method: Some("POST"),
        uri: Some("/"),
    }
);
client_case!(
    aws_json_11_static,
    aws_json_11_static_runner,
    aws_json_11_client,
    static_service(),
    RequestOverrides {
        extra_headers: &[
            ("x-amz-target", "PokemonService.GetServerStatistics"),
            ("content-type", "application/x-amz-json-1.1"),
            ("accept", "application/x-amz-json-1.1"),
        ],
        method: Some("POST"),
        uri: Some("/"),
    }
);

client_case!(
    rpcv2_cbor_legacy,
    rpcv2_cbor_legacy_runner,
    rpcv2_cbor_client,
    legacy_rpcv2_cbor_service()
);
client_case!(
    rpcv2_cbor_dynamic,
    rpcv2_cbor_dynamic_runner,
    rpcv2_cbor_client,
    dynamic_service()
);
client_case!(
    rpcv2_cbor_static,
    rpcv2_cbor_static_runner,
    rpcv2_cbor_client,
    static_service()
);

macro_rules! benchmark_cases {
    ($($case_name:literal => $runner:ident;)+) => {
        pub const CASES: &[BenchmarkCase] = &[
            $(
                BenchmarkCase {
                    name: $case_name,
                    make_runner: $runner,
                },
            )+
        ];
    };
}

benchmark_cases! {
    "restJson1/legacy" => rest_json1_legacy_runner;
    "restJson1/dynamic" => rest_json1_dynamic_runner;
    "restJson1/static" => rest_json1_static_runner;

    "restXml/legacy" => rest_xml_legacy_runner;
    "restXml/dynamic" => rest_xml_dynamic_runner;
    "restXml/static" => rest_xml_static_runner;

    "awsJson1_0/legacy" => aws_json_10_legacy_runner;
    "awsJson1_0/dynamic" => aws_json_10_dynamic_runner;
    "awsJson1_0/static" => aws_json_10_static_runner;

    "awsJson1_1/legacy" => aws_json_11_legacy_runner;
    "awsJson1_1/dynamic" => aws_json_11_dynamic_runner;
    "awsJson1_1/static" => aws_json_11_static_runner;

    "rpcv2Cbor/legacy" => rpcv2_cbor_legacy_runner;
    "rpcv2Cbor/dynamic" => rpcv2_cbor_dynamic_runner;
    "rpcv2Cbor/static" => rpcv2_cbor_static_runner;
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn all_cases_succeed() {
        for case in super::CASES {
            case.run().await;
        }
    }

    #[test]
    fn stable_case_names_are_exposed() {
        let names = super::case_names().collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "restJson1/legacy",
                "restJson1/dynamic",
                "restJson1/static",
                "restXml/legacy",
                "restXml/dynamic",
                "restXml/static",
                "awsJson1_0/legacy",
                "awsJson1_0/dynamic",
                "awsJson1_0/static",
                "awsJson1_1/legacy",
                "awsJson1_1/dynamic",
                "awsJson1_1/static",
                "rpcv2Cbor/legacy",
                "rpcv2Cbor/dynamic",
                "rpcv2Cbor/static",
            ]
        );
    }
}
