/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_credential_types::cache::{CredentialsCache, SharedCredentialsCache};
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_http::user_agent::{ApiMetadata, AwsUserAgent};
use aws_runtime::recursion_detection::RecursionDetectionInterceptor;
use aws_runtime::user_agent::UserAgentInterceptor;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::endpoint::Params;
use aws_sdk_s3::operation::list_objects_v2::{
    ListObjectsV2Error, ListObjectsV2Input, ListObjectsV2Output,
};
use aws_sdk_s3::primitives::SdkBody;
use aws_smithy_client::erase::DynConnector;
use aws_smithy_client::test_connection::TestConnection;
use aws_smithy_http::endpoint::SharedEndpointResolver;
use aws_smithy_runtime::client::connections::adapter::DynConnectorAdapter;
use aws_smithy_runtime::client::orchestrator::endpoints::DefaultEndpointResolver;
use aws_smithy_runtime_api::client::interceptors::error::ContextAttachedError;
use aws_smithy_runtime_api::client::interceptors::{Interceptor, InterceptorContext, Interceptors};
use aws_smithy_runtime_api::client::orchestrator::{
    BoxError, ConfigBagAccessors, Connection, HttpRequest, HttpResponse, RequestTime, TraceProbe,
};
use aws_smithy_runtime_api::client::retries::RetryClassifiers;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_runtime_api::config_bag::ConfigBag;
use aws_smithy_runtime_api::type_erasure::TypedBox;
use aws_types::region::SigningRegion;
use aws_types::SigningService;
use http::Uri;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

// TODO(orchestrator-test): unignore
#[ignore]
#[tokio::test]
async fn sra_test() {
    tracing_subscriber::fmt::init();

    let conn = TestConnection::new(vec![(
        http::Request::builder()
            .header("authorization", "AWS4-HMAC-SHA256 Credential=ANOTREAL/20210618/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=ae78f74d26b6b0c3a403d9e8cc7ec3829d6264a2b33db672bf2b151bbb901786")
            .uri("https://test-bucket.s3.us-east-1.amazonaws.com/?list-type=2&prefix=prefix~")
            .body(SdkBody::empty())
            .unwrap(),
        http::Response::builder().status(200).body("").unwrap(),
    )]);

    let config = aws_sdk_s3::Config::builder()
        .credentials_provider(Credentials::for_tests())
        .region(Region::new("us-east-1"))
        .http_connector(conn.clone())
        .build();
    let client = aws_sdk_s3::Client::from_conf(config);

    let _ = dbg!(
        client
            .list_objects_v2()
            .config_override(aws_sdk_s3::Config::builder().force_path_style(false))
            .bucket("test-bucket")
            .prefix("prefix~")
            .send_v2()
            .await
    );

    conn.assert_requests_match(&[]);
}

// TODO(orchestrator-test): replace with the above once runtime plugin config works
#[tokio::test]
async fn sra_manual_test() {
    tracing_subscriber::fmt::init();

    struct ManualServiceRuntimePlugin {
        connector: TestConnection<&'static str>,
        endpoint_resolver: SharedEndpointResolver<Params>,
        credentials_cache: SharedCredentialsCache,
    }

    impl RuntimePlugin for ManualServiceRuntimePlugin {
        fn configure(&self, cfg: &mut ConfigBag) -> Result<(), BoxError> {
            let http_auth_schemes =
                aws_smithy_runtime_api::client::orchestrator::HttpAuthSchemes::builder()
                    .auth_scheme(
                        aws_runtime::auth::sigv4::SCHEME_ID,
                        aws_runtime::auth::sigv4::SigV4HttpAuthScheme::new(),
                    )
                    .build();
            cfg.set_http_auth_schemes(http_auth_schemes);

            // Set an empty auth option resolver to be overridden by operations that need auth.
            cfg.set_auth_option_resolver(
                aws_smithy_runtime_api::client::auth::option_resolver::AuthOptionListResolver::new(
                    Vec::new(),
                ),
            );

            cfg.set_endpoint_resolver(DefaultEndpointResolver::new(self.endpoint_resolver.clone()));

            let params_builder = Params::builder()
                .set_region(Some("us-east-1".to_owned()))
                .set_endpoint(Some("https://s3.us-east-1.amazonaws.com/".to_owned()));
            cfg.put(params_builder);

            cfg.set_retry_strategy(
                aws_smithy_runtime::client::retries::strategy::StandardRetryStrategy::default(),
            );

            let connection: Box<dyn Connection> = Box::new(DynConnectorAdapter::new(
                DynConnector::new(self.connector.clone()),
            ));
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
            cfg.set_request_time(RequestTime::new(
                UNIX_EPOCH + Duration::from_secs(1624036048),
            ));

            cfg.put(ApiMetadata::new("unused", "unused"));
            cfg.put(AwsUserAgent::for_tests()); // Override the user agent with the test UA
            cfg.get::<Interceptors<HttpRequest, HttpResponse>>()
                .expect("interceptors set")
                .register_client_interceptor(Arc::new(UserAgentInterceptor::new()) as _)
                .register_client_interceptor(Arc::new(RecursionDetectionInterceptor::new()) as _);
            cfg.set_identity_resolvers(
                aws_smithy_runtime_api::client::identity::IdentityResolvers::builder()
                    .identity_resolver(
                        aws_runtime::auth::sigv4::SCHEME_ID,
                        aws_runtime::identity::credentials::CredentialsIdentityResolver::new(
                            self.credentials_cache.clone(),
                        ),
                    )
                    .build(),
            );
            Ok(())
        }
    }

    // This is a temporary operation runtime plugin until <Operation>EndpointParamsInterceptor and
    // <Operation>EndpointParamsFinalizerInterceptor have been fully implemented, in which case
    // `.with_operation_plugin(ManualOperationRuntimePlugin)` can be removed.
    struct ManualOperationRuntimePlugin;

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
                        .ok_or("missing endpoint params builder")?
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
                        .ok_or("missing endpoint params builder")?
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

    let conn = TestConnection::new(vec![(
                http::Request::builder()
                    .header("authorization", "AWS4-HMAC-SHA256 Credential=ANOTREAL/20210618/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=ae78f74d26b6b0c3a403d9e8cc7ec3829d6264a2b33db672bf2b151bbb901786")
                    .uri("https://test-bucket.s3.us-east-1.amazonaws.com/?list-type=2&prefix=prefix~")
                    .body(SdkBody::empty())
                    .unwrap(),
                http::Response::builder().status(200).body(r#"<?xml version="1.0" encoding="UTF-8"?>
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
"#).unwrap(),
            )]);

    let endpoint_resolver =
        SharedEndpointResolver::new(aws_sdk_s3::endpoint::DefaultResolver::new());
    let credentials_cache = SharedCredentialsCache::new(
        CredentialsCache::lazy()
            .create_cache(SharedCredentialsProvider::new(Credentials::for_tests())),
    );

    let service_runtime_plugin = ManualServiceRuntimePlugin {
        connector: conn.clone(),
        endpoint_resolver,
        credentials_cache,
    };

    let runtime_plugins = aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugins::new()
        .with_client_plugin(service_runtime_plugin)
        .with_operation_plugin(aws_sdk_s3::operation::list_objects_v2::ListObjectsV2::new())
        .with_operation_plugin(ManualOperationRuntimePlugin);

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
    let output = TypedBox::<ListObjectsV2Output>::assume_from(output)
        .expect("correct output type")
        .unwrap();
    dbg!(output);

    conn.assert_requests_match(&[]);
}
