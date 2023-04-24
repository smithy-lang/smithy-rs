/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[macro_use]
extern crate criterion;
use aws_credential_types::cache::{CredentialsCache, SharedCredentialsCache};
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_credential_types::Credentials;
use aws_sdk_s3 as s3;
use aws_smithy_client::erase::DynConnector;
use aws_smithy_client::test_connection::infallible_connection_fn;
use aws_smithy_http::endpoint::SharedEndpointResolver;
use aws_smithy_runtime_api::type_erasure::TypedBox;
use criterion::Criterion;
use s3::operation::list_objects_v2::{ListObjectsV2Error, ListObjectsV2Input, ListObjectsV2Output};

async fn middleware(client: &s3::Client) {
    client
        .list_objects_v2()
        .bucket("test-bucket")
        .prefix("prefix~")
        .send()
        .await
        .expect("successful execution");
}

async fn orchestrator(
    connector: &DynConnector,
    endpoint_resolver: SharedEndpointResolver<s3::endpoint::Params>,
    credentials_cache: SharedCredentialsCache,
) {
    let service_runtime_plugin = orchestrator::ManualServiceRuntimePlugin {
        connector: connector.clone(),
        endpoint_resolver: endpoint_resolver.clone(),
        credentials_cache: credentials_cache.clone(),
    };

    // TODO(enableNewSmithyRuntime): benchmark with `send_v2` directly once it works
    let runtime_plugins = aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugins::new()
        .with_client_plugin(service_runtime_plugin)
        .with_operation_plugin(aws_sdk_s3::operation::list_objects_v2::ListObjectsV2::new())
        .with_operation_plugin(orchestrator::ManualOperationRuntimePlugin);
    let input = ListObjectsV2Input::builder()
        .bucket("test-bucket")
        .prefix("prefix~")
        .build()
        .unwrap();
    let input = TypedBox::new(input).erase();
    let output = aws_smithy_runtime::client::orchestrator::invoke(input, &runtime_plugins)
        .await
        .map_err(|err| {
            err.map_service_error(|err| {
                TypedBox::<ListObjectsV2Error>::assume_from(err)
                    .expect("correct error type")
                    .unwrap()
            })
        })
        .unwrap();
    TypedBox::<ListObjectsV2Output>::assume_from(output)
        .expect("correct output type")
        .unwrap();
}

fn test_connection() -> DynConnector {
    infallible_connection_fn(|req| {
        assert_eq!(
            "https://test-bucket.s3.us-east-1.amazonaws.com/?list-type=2&prefix=prefix~",
            req.uri().to_string()
        );
        assert!(req.headers().contains_key("authorization"));
        http::Response::builder()
            .status(200)
            .body(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
    <Name>test-bucket</Name>
    <Prefix>prefix~</Prefix>
    <KeyCount>1</KeyCount>
    <MaxKeys>1000</MaxKeys>
    <IsTruncated>false</IsTruncated>
    <Contents>
        <Key>some-file.file</Key>
        <LastModified>2009-10-12T17:50:30.000Z</LastModified>
        <Size>434234</Size>
        <StorageClass>STANDARD</StorageClass>
    </Contents>
</ListBucketResult>
"#,
            )
            .unwrap()
    })
}

fn middleware_bench(c: &mut Criterion) {
    let conn = test_connection();
    let config = s3::Config::builder()
        .credentials_provider(s3::config::Credentials::for_tests())
        .region(s3::config::Region::new("us-east-1"))
        .http_connector(conn.clone())
        .build();
    let client = s3::Client::from_conf(config);
    c.bench_function("middleware", move |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async { middleware(&client).await })
    });
}

fn orchestrator_bench(c: &mut Criterion) {
    let conn = test_connection();
    let endpoint_resolver = SharedEndpointResolver::new(s3::endpoint::DefaultResolver::new());
    let credentials_cache = SharedCredentialsCache::new(
        CredentialsCache::lazy()
            .create_cache(SharedCredentialsProvider::new(Credentials::for_tests())),
    );

    c.bench_function("orchestrator", move |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                orchestrator(&conn, endpoint_resolver.clone(), credentials_cache.clone()).await
            })
    });
}

mod orchestrator {
    use aws_credential_types::cache::SharedCredentialsCache;
    use aws_http::user_agent::{ApiMetadata, AwsUserAgent};
    use aws_runtime::recursion_detection::RecursionDetectionInterceptor;
    use aws_runtime::user_agent::UserAgentInterceptor;
    use aws_sdk_s3::config::Region;
    use aws_sdk_s3::endpoint::Params;
    use aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Input;
    use aws_smithy_client::erase::DynConnector;
    use aws_smithy_http::endpoint::SharedEndpointResolver;
    use aws_smithy_runtime::client::connections::adapter::DynConnectorAdapter;
    use aws_smithy_runtime::client::orchestrator::endpoints::DefaultEndpointResolver;
    use aws_smithy_runtime_api::client::interceptors::{
        Interceptor, InterceptorContext, InterceptorError, Interceptors,
    };
    use aws_smithy_runtime_api::client::orchestrator::{
        BoxError, ConfigBagAccessors, Connection, HttpRequest, HttpResponse, TraceProbe,
    };
    use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
    use aws_smithy_runtime_api::config_bag::ConfigBag;
    use aws_types::region::SigningRegion;
    use aws_types::SigningService;
    use std::sync::Arc;

    pub struct ManualServiceRuntimePlugin {
        pub connector: DynConnector,
        pub endpoint_resolver: SharedEndpointResolver<Params>,
        pub credentials_cache: SharedCredentialsCache,
    }

    impl RuntimePlugin for ManualServiceRuntimePlugin {
        fn configure(&self, cfg: &mut ConfigBag) -> Result<(), BoxError> {
            let identity_resolvers =
                aws_smithy_runtime_api::client::orchestrator::IdentityResolvers::builder()
                    .identity_resolver(
                        aws_runtime::auth::sigv4::SCHEME_ID,
                        aws_runtime::identity::credentials::CredentialsIdentityResolver::new(
                            self.credentials_cache.clone(),
                        ),
                    )
                    .identity_resolver(
                        "anonymous",
                        aws_smithy_runtime_api::client::identity::AnonymousIdentityResolver::new(),
                    )
                    .build();
            cfg.set_identity_resolvers(identity_resolvers);

            let http_auth_schemes =
                aws_smithy_runtime_api::client::orchestrator::HttpAuthSchemes::builder()
                    .auth_scheme(
                        aws_runtime::auth::sigv4::SCHEME_ID,
                        aws_runtime::auth::sigv4::SigV4HttpAuthScheme::new(),
                    )
                    .build();
            cfg.set_http_auth_schemes(http_auth_schemes);

            cfg.set_auth_option_resolver(
                aws_smithy_runtime_api::client::auth::option_resolver::AuthOptionListResolver::new(
                    Vec::new(),
                ),
            );

            cfg.set_endpoint_resolver(DefaultEndpointResolver::new(self.endpoint_resolver.clone()));

            let params_builder = aws_sdk_s3::endpoint::Params::builder()
                .set_region(Some("us-east-1".to_owned()))
                .set_endpoint(Some("https://s3.us-east-1.amazonaws.com/".to_owned()));
            cfg.put(params_builder);

            cfg.set_retry_strategy(
                aws_smithy_runtime_api::client::retries::NeverRetryStrategy::new(),
            );

            let connection: Box<dyn Connection> =
                Box::new(DynConnectorAdapter::new(self.connector.clone()));
            cfg.set_connection(connection);

            cfg.set_trace_probe({
                #[derive(Debug)]
                struct StubTraceProbe;
                impl TraceProbe for StubTraceProbe {
                    fn dispatch_events(&self) {
                        // no-op
                    }
                }
                StubTraceProbe
            });

            cfg.put(SigningService::from_static("s3"));
            cfg.put(SigningRegion::from(Region::from_static("us-east-1")));

            cfg.put(ApiMetadata::new("unused", "unused"));
            cfg.put(AwsUserAgent::for_tests()); // Override the user agent with the test UA
            cfg.get::<Interceptors<HttpRequest, HttpResponse>>()
                .expect("interceptors set")
                .register_client_interceptor(Arc::new(UserAgentInterceptor::new()) as _)
                .register_client_interceptor(Arc::new(RecursionDetectionInterceptor::new()) as _);
            Ok(())
        }
    }

    // This is a temporary operation runtime plugin until <Operation>EndpointParamsInterceptor and
    // <Operation>EndpointParamsFinalizerInterceptor have been fully implemented, in which case
    // `.with_operation_plugin(ManualOperationRuntimePlugin)` can be removed.
    pub struct ManualOperationRuntimePlugin;

    impl RuntimePlugin for ManualOperationRuntimePlugin {
        fn configure(&self, cfg: &mut ConfigBag) -> Result<(), BoxError> {
            #[derive(Debug)]
            struct ListObjectsV2EndpointParamsInterceptor;
            impl Interceptor<HttpRequest, HttpResponse> for ListObjectsV2EndpointParamsInterceptor {
                fn read_before_execution(
                    &self,
                    context: &InterceptorContext<HttpRequest, HttpResponse>,
                    cfg: &mut ConfigBag,
                ) -> Result<(), BoxError> {
                    let input = context.input()?;
                    let input = input
                        .downcast_ref::<ListObjectsV2Input>()
                        .ok_or_else(|| "failed to downcast to ListObjectsV2Input")?;
                    let mut params_builder = cfg
                        .get::<aws_sdk_s3::endpoint::ParamsBuilder>()
                        .ok_or_else(|| "missing endpoint params builder")?
                        .clone();
                    params_builder = params_builder.set_bucket(input.bucket.clone());
                    cfg.put(params_builder);

                    Ok(())
                }
            }

            #[derive(Debug)]
            struct ListObjectsV2EndpointParamsFinalizerInterceptor;
            impl Interceptor<HttpRequest, HttpResponse> for ListObjectsV2EndpointParamsFinalizerInterceptor {
                fn read_before_execution(
                    &self,
                    _context: &InterceptorContext<HttpRequest, HttpResponse>,
                    cfg: &mut ConfigBag,
                ) -> Result<(), BoxError> {
                    let params_builder = cfg
                        .get::<aws_sdk_s3::endpoint::ParamsBuilder>()
                        .ok_or_else(|| "missing endpoint params builder")?
                        .clone();
                    let params = params_builder.build().map_err(|err| {
                        ContextAttachedError::new("endpoint params could not be built", err)
                    })?;
                    cfg.put(
                        aws_smithy_runtime_api::client::orchestrator::EndpointResolverParams::new(
                            params,
                        ),
                    );

                    Ok(())
                }
            }

            cfg.get::<Interceptors<HttpRequest, HttpResponse>>()
                .expect("interceptors set")
                .register_operation_interceptor(
                    Arc::new(ListObjectsV2EndpointParamsInterceptor) as _
                )
                .register_operation_interceptor(Arc::new(
                    ListObjectsV2EndpointParamsFinalizerInterceptor,
                ) as _);
            Ok(())
        }
    }
}

criterion_group!(benches, middleware_bench, orchestrator_bench);
criterion_main!(benches);
