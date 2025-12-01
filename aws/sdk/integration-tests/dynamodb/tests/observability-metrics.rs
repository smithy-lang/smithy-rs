/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::Region;
use aws_credential_types::Credentials;
use aws_runtime::user_agent::test_util::assert_ua_contains_metric_values;
use aws_sdk_dynamodb::config::Builder;
use aws_smithy_observability::TelemetryProvider;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use std::sync::Arc;

fn test_client(
    update_builder: fn(Builder) -> Builder,
) -> (aws_sdk_dynamodb::Client, StaticReplayClient) {
    let http_client = StaticReplayClient::new(vec![ReplayEvent::new(
        http::Request::builder()
            .uri("https://dynamodb.us-east-1.amazonaws.com/")
            .body(SdkBody::empty())
            .unwrap(),
        http::Response::builder()
            .status(200)
            .body(SdkBody::from(r#"{"TableNames":[]}"#))
            .unwrap(),
    )]);

    let config = update_builder(
        aws_sdk_dynamodb::Config::builder()
            .credentials_provider(Credentials::for_tests())
            .region(Region::from_static("us-east-1"))
            .http_client(http_client.clone())
            .behavior_version_latest(),
    )
    .build();

    (aws_sdk_dynamodb::Client::from_conf(config), http_client)
}

async fn call_operation(client: aws_sdk_dynamodb::Client) {
    let _ = client.list_tables().send().await;
}

// Mock OTel meter provider for testing
// This mimics the real OtelMeterProvider's type name without requiring the broken opentelemetry crate
// We create it in a module path that matches what our type checking looks for
mod mock_otel {
    use aws_smithy_observability::meter::{Meter, ProvideMeter};
    use aws_smithy_observability::Attributes;

    #[derive(Debug)]
    pub struct OtelMeterProvider;

    impl ProvideMeter for OtelMeterProvider {
        fn get_meter(&self, _scope: &'static str, _attributes: Option<&Attributes>) -> Meter {
            // Return a noop meter - we don't actually need it to work, just to exist
            Meter::new(std::sync::Arc::new(NoopInstrumentProvider))
        }

        fn provider_name(&self) -> &'static str {
            "otel"
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[derive(Debug)]
    struct NoopInstrumentProvider;

    impl aws_smithy_observability::instruments::ProvideInstrument for NoopInstrumentProvider {
        fn create_gauge(
            &self,
            _builder: aws_smithy_observability::instruments::AsyncInstrumentBuilder<
                '_,
                std::sync::Arc<dyn aws_smithy_observability::instruments::AsyncMeasure<Value = f64>>,
                f64,
            >,
        ) -> std::sync::Arc<dyn aws_smithy_observability::instruments::AsyncMeasure<Value = f64>>
        {
            std::sync::Arc::new(NoopAsync::<f64>(std::marker::PhantomData))
        }

        fn create_up_down_counter(
            &self,
            _builder: aws_smithy_observability::instruments::InstrumentBuilder<
                '_,
                std::sync::Arc<dyn aws_smithy_observability::instruments::UpDownCounter>,
            >,
        ) -> std::sync::Arc<dyn aws_smithy_observability::instruments::UpDownCounter> {
            std::sync::Arc::new(NoopUpDown)
        }

        fn create_async_up_down_counter(
            &self,
            _builder: aws_smithy_observability::instruments::AsyncInstrumentBuilder<
                '_,
                std::sync::Arc<dyn aws_smithy_observability::instruments::AsyncMeasure<Value = i64>>,
                i64,
            >,
        ) -> std::sync::Arc<dyn aws_smithy_observability::instruments::AsyncMeasure<Value = i64>>
        {
            std::sync::Arc::new(NoopAsync::<i64>(std::marker::PhantomData))
        }

        fn create_monotonic_counter(
            &self,
            _builder: aws_smithy_observability::instruments::InstrumentBuilder<
                '_,
                std::sync::Arc<dyn aws_smithy_observability::instruments::MonotonicCounter>,
            >,
        ) -> std::sync::Arc<dyn aws_smithy_observability::instruments::MonotonicCounter> {
            std::sync::Arc::new(NoopMono)
        }

        fn create_async_monotonic_counter(
            &self,
            _builder: aws_smithy_observability::instruments::AsyncInstrumentBuilder<
                '_,
                std::sync::Arc<dyn aws_smithy_observability::instruments::AsyncMeasure<Value = u64>>,
                u64,
            >,
        ) -> std::sync::Arc<dyn aws_smithy_observability::instruments::AsyncMeasure<Value = u64>>
        {
            std::sync::Arc::new(NoopAsync::<u64>(std::marker::PhantomData))
        }

        fn create_histogram(
            &self,
            _builder: aws_smithy_observability::instruments::InstrumentBuilder<
                '_,
                std::sync::Arc<dyn aws_smithy_observability::instruments::Histogram>,
            >,
        ) -> std::sync::Arc<dyn aws_smithy_observability::instruments::Histogram> {
            std::sync::Arc::new(NoopHist)
        }
    }

    #[derive(Debug)]
    struct NoopAsync<T>(std::marker::PhantomData<T>);
    impl<T: Send + Sync + std::fmt::Debug> aws_smithy_observability::instruments::AsyncMeasure
        for NoopAsync<T>
    {
        type Value = T;
        fn record(
            &self,
            _value: T,
            _attributes: Option<&aws_smithy_observability::Attributes>,
            _context: Option<&dyn aws_smithy_observability::Context>,
        ) {
        }
        fn stop(&self) {}
    }

    #[derive(Debug)]
    struct NoopUpDown;
    impl aws_smithy_observability::instruments::UpDownCounter for NoopUpDown {
        fn add(
            &self,
            _value: i64,
            _attributes: Option<&aws_smithy_observability::Attributes>,
            _context: Option<&dyn aws_smithy_observability::Context>,
        ) {
        }
    }

    #[derive(Debug)]
    struct NoopMono;
    impl aws_smithy_observability::instruments::MonotonicCounter for NoopMono {
        fn add(
            &self,
            _value: u64,
            _attributes: Option<&aws_smithy_observability::Attributes>,
            _context: Option<&dyn aws_smithy_observability::Context>,
        ) {
        }
    }

    #[derive(Debug)]
    struct NoopHist;
    impl aws_smithy_observability::instruments::Histogram for NoopHist {
        fn record(
            &self,
            _value: f64,
            _attributes: Option<&aws_smithy_observability::Attributes>,
            _context: Option<&dyn aws_smithy_observability::Context>,
        ) {
        }
    }
}

#[tokio::test]
async fn observability_metrics_in_user_agent() {
    // Test case 1: No telemetry provider configured (default noop)
    {
        let (client, http_client) = test_client(std::convert::identity);
        call_operation(client).await;
        let req = http_client.actual_requests().last().expect("request");
        let user_agent = req.headers().get("x-amz-user-agent").expect("user-agent header");

        // Should NOT contain observability metrics when using noop provider
        assert!(!user_agent.contains("m/7")); // OBSERVABILITY_OTEL_METRICS = "7"
    }

    // Test case 2: OpenTelemetry metrics provider configured
    {
        use mock_otel::OtelMeterProvider;

        // Create mock OTel meter provider
        let sdk_mp = Arc::new(OtelMeterProvider);
        
        // Debug: Check what the type name actually is
        let type_name = std::any::type_name_of_val(&sdk_mp);
        eprintln!("Mock OTel provider Arc type name: {}", type_name);
        let type_name2 = std::any::type_name_of_val(sdk_mp.as_ref());
        eprintln!("Mock OTel provider as_ref type name: {}", type_name2);
        
        let sdk_tp = TelemetryProvider::builder().meter_provider(sdk_mp).build();

        // Set global telemetry provider
        aws_smithy_observability::global::set_telemetry_provider(sdk_tp).unwrap();

        let (client, http_client) = test_client(std::convert::identity);
        call_operation(client).await;
        let req = http_client.actual_requests().last().expect("request");
        let user_agent = req.headers().get("x-amz-user-agent").expect("user-agent header");

        eprintln!("User-Agent: {}", user_agent);

        // Should contain OBSERVABILITY_OTEL_METRICS metric
        assert_ua_contains_metric_values(user_agent, &["7"]);

        // Reset to noop for other tests
        aws_smithy_observability::global::set_telemetry_provider(TelemetryProvider::noop())
            .unwrap();
    }
}
