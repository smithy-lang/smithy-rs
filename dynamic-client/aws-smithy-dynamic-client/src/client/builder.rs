/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// The builder for a dynamic client, and the steps of `build()`.

use super::DynamicClient;
use crate::config::{self, Applies, Put};
use crate::error::{BuildError, MissingHttpClient};
use crate::protocol::{detect_protocol, protocol_traits, AWS_QUERY};
use aws_smithy_lang::endpoint_rules::{EndpointBdd, EndpointRuleSet, Parameters};
use aws_smithy_lang::runtime_schema::{Materialization, RuntimeSchemaSet};
use aws_smithy_lang::shape::ServiceView;
use aws_smithy_lang::traversal::Walker;
use aws_smithy_lang::{Model, ShapeId};
use aws_smithy_runtime::client::defaults::{default_plugins, DefaultPluginParams};
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
use aws_smithy_runtime_api::client::runtime_components::{
    RuntimeComponentsBuilder, SharedConfigValidator,
};
use aws_smithy_runtime_api::client::runtime_plugin::{RuntimePlugins, StaticRuntimePlugin};
use aws_smithy_schema::protocol::{
    ServiceShapeName, ServiceShapeNamespace, ServiceVersion, ServiceXmlNamespace,
};
use aws_smithy_types::config_bag::{ConfigBag, Layer};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

/// A builder for a [`DynamicClient`].
#[derive(Clone, Debug, Default)]
pub struct DynamicClientBuilder {
    model_file: Option<PathBuf>,
    service: Option<ShapeId>,
    config: config::Builder,
}

impl DynamicClientBuilder {
    /// Sets the path of the Smithy JSON AST file to load the model from.
    ///
    /// [`build`](DynamicClientBuilder::build) reads the file.
    pub fn model_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.model_file = Some(path.into());
        self
    }

    /// Sets the shape ID of the service to call.
    ///
    /// A model with one service needs no service ID. A model with several services needs one.
    pub fn service(mut self, id: ShapeId) -> Self {
        self.service = Some(id);
        self
    }

    /// Layers a config over the config set so far. Each field the new config sets wins.
    ///
    /// Takes a [`Config`](config::Config) or a [`config::Builder`].
    pub fn config(mut self, config: impl Into<config::Builder>) -> Self {
        self.config = self.config.merge(config.into());
        self
    }

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// Fails when the builder has no model, or when the model file fails to load. Also fails
    /// when the model has no service, or has several and none is specified. Also fails when
    /// the model has no service with the given ID, or when the service's shapes are invalid.
    /// Also fails when no protocol is set and no built-in protocol matches a protocol trait
    /// of the service. Also fails when the config has no behavior version, or when the
    /// awsQuery protocol is used and the service has no version. Also fails on an invalid
    /// config, for example with retries and no sleep implementation, or with no HTTP client.
    pub fn build(self) -> Result<DynamicClient, BuildError> {
        let path = self.model_file.ok_or_else(BuildError::missing_model)?;
        let behavior_version = self
            .config
            .behavior_version
            .ok_or_else(BuildError::missing_behavior_version)?;
        // TODO(dynamic-client): Also load Smithy IDL. Pick the loader by file extension, `.json` or
        // `.smithy`, so `model_file(..)` stays the only setter. A fallback to IDL after a JSON
        // failure would report an IDL syntax error for a broken JSON file.
        let model = Model::from_json_file(&path)
            .map_err(|source| BuildError::model_load_failed(path, source))?;
        let service_view = select_service(&model, self.service)?;
        let service = service_view.id().clone();
        // A lazy set validates the service's shapes now, and builds each schema on first use.
        let schemas = RuntimeSchemaSet::builder(&model)
            .for_service(&service)
            .materialization(Materialization::Lazy)
            .build()
            .map_err(BuildError::schema_build_failed)?;
        let protocol = match self.config.protocol {
            Some(protocol) => protocol,
            None => detect_protocol(&service_view).ok_or_else(|| {
                BuildError::protocol_not_found(
                    service.clone(),
                    protocol_traits(&model, &service_view),
                )
            })?,
        };
        // An awsQuery request carries the service version as its `Version` parameter.
        if protocol.protocol_id().to_string() == AWS_QUERY.as_str()
            && service_view.version().is_none()
        {
            return Err(BuildError::missing_service_version(service));
        }
        let mut components = RuntimeComponentsBuilder::new("DynamicClientConfig");
        components.set_http_client(self.config.http_client);
        components.set_sleep_impl(self.config.sleep_impl);
        components.set_time_source(self.config.time_source);
        components.set_identity_cache(self.config.identity_cache);
        for retry_classifier in self.config.retry_classifiers {
            components.push_retry_classifier(retry_classifier);
        }
        for interceptor in self.config.interceptors {
            components.push_interceptor(interceptor);
        }
        // TODO(dynamic-client): Also set the credentials provider for SigV4a, for a model that
        // uses SigV4a, as generated clients do.
        #[cfg(feature = "aws")]
        if let Some(credentials_provider) = self.config.credentials_provider {
            components
                .set_identity_resolver(aws_runtime::auth::sigv4::SCHEME_ID, credentials_provider);
        }
        components.push_config_validator(SharedConfigValidator::base_client_config_fn(
            validate_http_client,
        ));
        let mut layer = Layer::new("DynamicClientConfig");
        layer.store_put(protocol);
        store_service_facts(&mut layer, &service_view);
        if let Some(retry_config) = self.config.retry_config {
            layer.store_put(retry_config);
        }
        if let Some(timeout_config) = self.config.timeout_config {
            layer.store_put(timeout_config);
        }
        if let Some(retry_partition) = self.config.retry_partition {
            layer.store_put(retry_partition);
        }
        if let Some(stalled_stream_protection) = self.config.stalled_stream_protection {
            layer.store_put(stalled_stream_protection);
        }
        #[cfg(feature = "aws")]
        if let Some(region) = self.config.region {
            layer.store_put(region);
        }
        #[cfg(feature = "aws")]
        if let Some(invocation_id_generator) = self.config.invocation_id_generator {
            layer.store_put(invocation_id_generator);
        }
        #[cfg(feature = "aws")]
        if let Some(disable_clock_skew_correction) = self.config.disable_clock_skew_correction {
            layer.store_put(
                aws_runtime::service_clock_skew::DisableClockSkewCorrection::from(
                    disable_clock_skew_correction,
                ),
            );
        }
        let mut runtime_plugins =
            runtime_plugins(components, layer, behavior_version, service.name());
        if !self.config.puts.is_empty() {
            let mut puts = Layer::new("DynamicClientPuts");
            for put in &self.config.puts {
                (put.store)(&mut puts);
            }
            runtime_plugins = runtime_plugins
                .with_client_plugin(StaticRuntimePlugin::new().with_config(puts.freeze()));
        }
        let runtime_plugins = runtime_plugins.with_client_plugins(self.config.runtime_plugins);
        // The same check generated clients run when they are created.
        let mut cfg = ConfigBag::base();
        let components = runtime_plugins
            .apply_client_configuration(&mut cfg)
            .map_err(BuildError::invalid_config)?;
        components
            .validate_base_client_config(&cfg)
            .map_err(BuildError::invalid_config)?;
        // Runs after the last step that can fail, so a failed build logs no warning.
        for key in unused_keys(&service_view, &self.config.puts) {
            tracing::warn!(
                key,
                service = %service,
                "the service does not use this config key, so its value has no effect"
            );
        }
        Ok(DynamicClient {
            schemas: Arc::new(schemas),
            runtime_plugins,
        })
    }
}

// Returns the type names of the keys set by `put` that the service never uses, each once. A key
// is used when any of its `APPLIES` conditions holds, or when it has none.
fn unused_keys(service: &ServiceView<'_>, keys: &[Put]) -> Vec<&'static str> {
    if keys.is_empty() {
        return Vec::new();
    }
    let conditions = || keys.iter().flat_map(|key| key.applies);
    // The wanted traits not found yet on a shape the service reaches. The walk stops once every
    // wanted trait is found.
    let mut missing_traits: HashSet<&ShapeId> = conditions()
        .filter_map(|applies| match applies {
            Applies::Trait(id) => Some(id),
            _ => None,
        })
        .collect();
    if !missing_traits.is_empty() {
        for shape in Walker::new().walk(*service) {
            missing_traits.retain(|id| !shape.has_trait(id));
            if missing_traits.is_empty() {
                break;
            }
        }
    }
    let wanted_built_ins: HashSet<&str> = conditions()
        .filter_map(|applies| match applies {
            Applies::EndpointBuiltIn(built_in) => Some(built_in.as_str()),
            _ => None,
        })
        .collect();
    // TODO(dynamic-client): Reuse the endpoint rules decoded for endpoint resolution, once
    // `build()` decodes them.
    let missing_built_ins = if wanted_built_ins.is_empty() {
        Some(wanted_built_ins)
    } else {
        missing_built_ins(service, wanted_built_ins)
    };
    let mut seen = HashSet::new();
    keys.iter()
        .filter(|key| seen.insert(key.type_id))
        .filter(|key| {
            !key.applies.is_empty()
                && !key.applies.iter().any(|applies| match applies {
                    Applies::Trait(id) => !missing_traits.contains(id),
                    Applies::EndpointBuiltIn(built_in) => missing_built_ins
                        .as_ref()
                        .is_none_or(|missing| !missing.contains(built_in.as_str())),
                    // A condition added later is treated as holding, so it never warns.
                    _ => true,
                })
        })
        .map(|key| key.name)
        .collect()
}

// Returns the names in `wanted` that no parameter of the service's endpoint rules declares as its
// `builtIn`. The BDD form is read first, then the rule-tree form. Returns `None` when the rules
// fail to decode, so no built-in key is reported. Decode errors are reported by endpoint
// resolution.
fn missing_built_ins<'k>(
    service: &ServiceView<'_>,
    mut wanted: HashSet<&'k str>,
) -> Option<HashSet<&'k str>> {
    let mut remove_declared = |parameters: &Parameters| {
        for built_in in parameters
            .iter()
            .filter_map(|parameter| parameter.built_in())
        {
            wanted.remove(built_in);
        }
    };
    let traits = service.traits();
    if let Some(bdd) = traits.get_as::<EndpointBdd>().ok()? {
        remove_declared(bdd.parameters());
    } else if let Some(rule_set) = traits.get_as::<EndpointRuleSet>().ok()? {
        remove_declared(rule_set.parameters());
    }
    Some(wanted)
}

// Fails when no HTTP client is set after every plugin is applied. A generated client finds a
// missing HTTP client only when it sends a request.
fn validate_http_client(
    components: &RuntimeComponentsBuilder,
    _cfg: &ConfigBag,
) -> Result<(), BoxError> {
    match components.http_client() {
        Some(_) => Ok(()),
        None => Err(MissingHttpClient.into()),
    }
}

// Stores the service facts that protocols read from the config, as generated clients store them.
// A generated client stores an empty version when the model has none.
fn store_service_facts(layer: &mut Layer, service: &ServiceView<'_>) {
    let id = service.id();
    layer.store_put(ServiceShapeName::new(id.name().to_owned()));
    layer.store_put(ServiceShapeNamespace::new(id.namespace().to_owned()));
    layer.store_put(ServiceVersion::new(
        service.version().unwrap_or_default().to_owned(),
    ));
    let xml_namespace = service
        .get_trait(&XML_NAMESPACE)
        .and_then(|value| value.as_object());
    if let Some(xml_namespace) = xml_namespace {
        let uri = xml_namespace.get("uri").and_then(|value| value.as_str());
        let prefix = xml_namespace.get("prefix").and_then(|value| value.as_str());
        layer.store_put(ServiceXmlNamespace::new(
            uri.unwrap_or_default().to_owned(),
            prefix.map(|prefix| prefix.to_owned().into()),
        ));
    }
}

// The default plugins, then a plugin for the client's config, as generated clients build them.
fn runtime_plugins(
    components: RuntimeComponentsBuilder,
    layer: Layer,
    behavior_version: BehaviorVersion,
    service_name: &str,
) -> RuntimePlugins {
    RuntimePlugins::new()
        .with_client_plugins(default_plugins(
            DefaultPluginParams::new()
                .with_retry_partition_name(service_name.to_owned())
                .with_behavior_version(behavior_version),
        ))
        .with_client_plugin(
            StaticRuntimePlugin::new()
                .with_config(layer.freeze())
                .with_runtime_components(components),
        )
}

const XML_NAMESPACE: ShapeId = ShapeId::from_static("smithy.api#xmlNamespace");

// Picks the service specified by the builder, or the model's only service. A specified service
// is looked up by ID, so only the default case walks the model's shapes.
fn select_service(
    model: &Model,
    requested: Option<ShapeId>,
) -> Result<ServiceView<'_>, BuildError> {
    if let Some(id) = requested {
        return model
            .get_shape(&id)
            .and_then(|shape| shape.as_shape())
            .and_then(|shape| shape.as_service())
            .ok_or_else(|| BuildError::unknown_service(id, service_ids(model)));
    }
    let mut services = model.services();
    match (services.next(), services.next()) {
        (Some(only), None) => Ok(only),
        (None, _) => Err(BuildError::no_service()),
        (Some(_), Some(_)) => Err(BuildError::ambiguous_service(service_ids(model))),
    }
}

// The IDs of the model's services, sorted, for the service selection errors.
fn service_ids(model: &Model) -> Vec<ShapeId> {
    let mut services: Vec<ShapeId> = model.services().map(|s| s.id().clone()).collect();
    services.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    services
}

#[cfg(test)]
mod tests {
    use crate::config::{
        Applies, BehaviorVersion, Config, ConfigKey, EndpointBuiltIn, RetryConfig, RetryPartition,
        StalledStreamProtectionConfig, TimeoutConfig,
    };
    use crate::error::BuildErrorKind;
    #[cfg(not(feature = "default-https-client"))]
    use crate::error::MissingHttpClient;
    use crate::BuildError;
    use crate::DynamicClient;
    use aws_smithy_async::test_util::instant_time_and_sleep;
    use aws_smithy_http_client::test_util::NeverClient;
    use aws_smithy_lang::runtime_schema::Materialization;
    use aws_smithy_lang::{Model, ShapeId};
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::identity::{
        IdentityFuture, ResolveCachedIdentity, SharedIdentityResolver,
    };
    use aws_smithy_runtime_api::client::interceptors::context::InterceptorContext;
    use aws_smithy_runtime_api::client::interceptors::Intercept;
    use aws_smithy_runtime_api::client::retries::classifiers::{ClassifyRetry, RetryAction};
    use aws_smithy_runtime_api::client::runtime_components::{
        RuntimeComponents, RuntimeComponentsBuilder,
    };
    use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
    use aws_smithy_schema::protocol::{
        ServiceShapeName, ServiceShapeNamespace, ServiceVersion, ServiceXmlNamespace,
    };
    use aws_smithy_types::config_bag::{
        ConfigBag, FrozenLayer, Layer, Storable, StoreAppend, StoreReplace,
    };
    use std::collections::HashSet;
    use std::fmt;
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    fn model(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/models")
            .join(name)
    }

    fn id(id: &str) -> ShapeId {
        ShapeId::new(id).unwrap()
    }

    // A builder with a behavior version, a test HTTP client, and a test sleep. Every client needs
    // a behavior version, and the other two keep the tests independent of the default features.
    fn builder() -> super::DynamicClientBuilder {
        let (_, sleep) = instant_time_and_sleep(UNIX_EPOCH);
        DynamicClient::builder().config(
            Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .http_client(NeverClient::new())
                .sleep_impl(sleep),
        )
    }

    fn build_error(file: &str, service: Option<&str>) -> BuildError {
        let mut builder = builder().model_file(model(file));
        if let Some(service) = service {
            builder = builder.service(id(service));
        }
        builder.build().unwrap_err()
    }

    #[test]
    fn fails_without_a_model() {
        let err = DynamicClient::builder().build().unwrap_err();
        assert!(matches!(err.kind(), BuildErrorKind::MissingModel));
    }

    #[test]
    fn fails_on_a_missing_file() {
        let path = model("missing.json");
        let err = build_error("missing.json", None);
        assert!(
            matches!(err.kind(), BuildErrorKind::ModelLoadFailed { path: p, .. } if *p == path)
        );
    }

    #[test]
    fn selects_the_only_service() {
        let client = builder().model_file(model("weather.json")).build().unwrap();
        assert_eq!("example.weather#Weather", client.service().as_str());
    }

    #[test]
    fn selects_the_specified_service() {
        let client = builder()
            .model_file(model("two-services.json"))
            .service(id("example.cafe#Cafe"))
            .build()
            .unwrap();
        assert_eq!("example.cafe#Cafe", client.service().as_str());
    }

    #[test]
    fn fails_on_a_model_with_no_service() {
        let err = build_error("no-service.json", None);
        assert!(matches!(err.kind(), BuildErrorKind::NoService));
    }

    #[test]
    fn fails_on_several_services_without_a_service_id() {
        let err = build_error("two-services.json", None);
        let expected = vec![id("example.cafe#Cafe"), id("example.weather#Weather")];
        assert!(
            matches!(err.kind(), BuildErrorKind::AmbiguousService { services } if *services == expected)
        );
    }

    #[test]
    fn fails_on_an_unknown_service_id() {
        let err = build_error("two-services.json", Some("example.cafe#Bakery"));
        let expected = vec![id("example.cafe#Cafe"), id("example.weather#Weather")];
        assert!(matches!(
            err.kind(),
            BuildErrorKind::UnknownService { id: unknown, services }
                if *unknown == id("example.cafe#Bakery") && *services == expected
        ));
    }

    #[test]
    fn builds_schemas_lazily_for_dynamodb() {
        let dynamodb = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../aws/sdk/aws-models/dynamodb.json");
        let client = builder().model_file(dynamodb).build().unwrap();
        assert_eq!(Materialization::Lazy, client.schemas.materialization());
        assert_eq!(
            "com.amazonaws.dynamodb#DynamoDB_20120810",
            client.service().as_str()
        );
        assert!(client
            .schemas
            .operation_ids()
            .any(|op| op.as_str() == "com.amazonaws.dynamodb#ListTables"));
    }

    #[test]
    fn fails_on_invalid_service_shapes() {
        let err = build_error("bad-schema.json", None);
        assert!(matches!(
            err.kind(),
            BuildErrorKind::SchemaBuildFailed { .. }
        ));
    }

    #[test]
    fn fails_on_a_service_id_of_another_shape() {
        let err = build_error("no-service.json", Some("example.weather#Forecast"));
        assert!(matches!(
            err.kind(),
            BuildErrorKind::UnknownService { id: unknown, services }
                if *unknown == id("example.weather#Forecast") && services.is_empty()
        ));
    }

    fn protocols(service: &str) -> super::DynamicClientBuilder {
        builder()
            .model_file(model("protocols.json"))
            .service(id(service))
    }

    #[test]
    fn detects_the_protocol_from_the_service_trait() {
        let dynamodb = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../aws/sdk/aws-models/dynamodb.json");
        let client = builder().model_file(dynamodb).build().unwrap();
        assert_eq!("aws.protocols#awsJson1_0", client.protocol_id());
    }

    #[test]
    fn picks_the_first_protocol_in_priority_order() {
        // The model lists `restJson1` first, and `rpcv2Cbor` has the higher priority.
        let client = protocols("example.protocols#RestJsonAndCbor")
            .build()
            .unwrap();
        assert_eq!("smithy.protocols#rpcv2Cbor", client.protocol_id());
    }

    #[test]
    fn uses_the_explicit_protocol() {
        let client =
            protocols("example.protocols#RestJsonAndCbor")
                .config(Config::builder().protocol(
                    aws_smithy_json::protocol::aws_rest_json_1::AwsRestJsonProtocol::new(),
                ))
                .build()
                .unwrap();
        assert_eq!("aws.protocols#restJson1", client.protocol_id());
    }

    #[test]
    fn fails_when_no_built_in_protocol_matches_a_known_protocol_trait() {
        let err = protocols("example.protocols#Ec2Query").build().unwrap_err();
        assert!(matches!(
            err.kind(),
            BuildErrorKind::ProtocolNotFound { traits, .. }
                if *traits == vec![id("aws.protocols#ec2Query")]
        ));
    }

    #[test]
    fn fails_when_no_built_in_protocol_matches_a_defined_protocol_trait() {
        let err = protocols("example.protocols#WireOnly").build().unwrap_err();
        assert!(matches!(
            err.kind(),
            BuildErrorKind::ProtocolNotFound { traits, .. }
                if *traits == vec![id("example.protocols#wire")]
        ));
    }

    #[test]
    fn fails_on_a_service_with_no_protocol_trait() {
        let err = protocols("example.protocols#NoProtocol")
            .build()
            .unwrap_err();
        assert!(matches!(
            err.kind(),
            BuildErrorKind::ProtocolNotFound { service, traits }
                if *service == id("example.protocols#NoProtocol") && traits.is_empty()
        ));
    }

    #[test]
    fn fails_without_a_behavior_version() {
        let err = DynamicClient::builder()
            .model_file(model("weather.json"))
            .build()
            .unwrap_err();
        assert!(matches!(err.kind(), BuildErrorKind::MissingBehaviorVersion));
    }

    #[test]
    fn a_later_config_wins_for_the_fields_it_sets() {
        // The second config sets no behavior version, so the first one's stays.
        let client =
            builder()
                .model_file(model("weather.json"))
                .config(Config::builder().protocol(
                    aws_smithy_json::protocol::aws_rest_json_1::AwsRestJsonProtocol::new(),
                ))
                .build()
                .unwrap();
        assert_eq!("aws.protocols#restJson1", client.protocol_id());
    }

    #[test]
    fn fails_on_aws_query_without_a_service_version() {
        let err = protocols("example.protocols#QueryWithoutVersion")
            .build()
            .unwrap_err();
        assert!(matches!(
            err.kind(),
            BuildErrorKind::MissingServiceVersion { service }
                if *service == id("example.protocols#QueryWithoutVersion")
        ));
    }

    #[cfg(all(feature = "default-https-client", feature = "rt-tokio"))]
    #[test]
    fn uses_the_default_http_client_and_sleep_when_none_is_set() {
        let client = DynamicClient::builder()
            .model_file(model("weather.json"))
            .config(Config::builder().behavior_version(BehaviorVersion::latest()))
            .build()
            .unwrap();
        let (_, components) = client.client_config();
        assert!(components.http_client().is_some());
        assert!(components.sleep_impl().is_some());
    }

    #[cfg(not(feature = "default-https-client"))]
    #[test]
    fn fails_without_an_http_client_and_without_the_default() {
        let (_, sleep) = instant_time_and_sleep(UNIX_EPOCH);
        let err = DynamicClient::builder()
            .model_file(model("weather.json"))
            .config(
                Config::builder()
                    .behavior_version(BehaviorVersion::latest())
                    .sleep_impl(sleep),
            )
            .build()
            .unwrap_err();
        assert!(matches!(err.kind(), BuildErrorKind::InvalidConfig { .. }));
        let source = std::error::Error::source(&err).expect("the validator error is the source");
        assert!(source.downcast_ref::<MissingHttpClient>().is_some());
    }

    #[test]
    fn stores_the_service_facts_in_the_config() {
        let client = protocols("example.protocols#Facts").build().unwrap();
        let (cfg, _) = client.client_config();
        assert_eq!("Facts", cfg.load::<ServiceShapeName>().unwrap().as_str());
        assert_eq!(
            "example.protocols",
            cfg.load::<ServiceShapeNamespace>().unwrap().as_str()
        );
        assert_eq!("2024-01-01", cfg.load::<ServiceVersion>().unwrap().as_str());
        let xml_namespace = cfg.load::<ServiceXmlNamespace>().unwrap();
        assert_eq!("https://example.com/facts", xml_namespace.uri());
        assert_eq!(Some("f"), xml_namespace.prefix());
    }

    #[test]
    fn stores_an_empty_version_and_no_xml_namespace_when_the_model_has_none() {
        let client = protocols("example.protocols#FactsWithoutVersion")
            .build()
            .unwrap();
        let (cfg, _) = client.client_config();
        assert_eq!("", cfg.load::<ServiceVersion>().unwrap().as_str());
        assert!(cfg.load::<ServiceXmlNamespace>().is_none());
    }

    #[test]
    fn uses_the_time_source_from_the_config() {
        let (time_source, _) = instant_time_and_sleep(UNIX_EPOCH + Duration::from_secs(42));
        let client = builder()
            .model_file(model("weather.json"))
            .config(Config::builder().time_source(time_source))
            .build()
            .unwrap();
        let (_, components) = client.client_config();
        let now = components.time_source().unwrap().now();
        assert_eq!(UNIX_EPOCH + Duration::from_secs(42), now);
    }

    #[derive(Debug)]
    struct NamedInterceptor(&'static str);

    impl Intercept for NamedInterceptor {
        fn name(&self) -> &'static str {
            self.0
        }
    }

    #[derive(Debug)]
    struct TestClassifier;

    impl ClassifyRetry for TestClassifier {
        fn classify_retry(&self, _ctx: &InterceptorContext) -> RetryAction {
            RetryAction::NoActionIndicated
        }

        fn name(&self) -> &'static str {
            "TestClassifier"
        }
    }

    // A runtime plugin that sets a retry config with `max_attempts` attempts.
    #[derive(Debug)]
    struct RetryPlugin(u32);

    impl RuntimePlugin for RetryPlugin {
        fn config(&self) -> Option<FrozenLayer> {
            let mut layer = Layer::new("RetryPlugin");
            layer.store_put(RetryConfig::standard().with_max_attempts(self.0));
            Some(layer.freeze())
        }
    }

    #[test]
    fn stores_the_retry_timeout_and_partition_settings() {
        let timeout_config = TimeoutConfig::builder()
            .operation_timeout(Duration::from_secs(5))
            .build();
        let client = builder()
            .model_file(model("weather.json"))
            .config(
                Config::builder()
                    .retry_config(RetryConfig::standard().with_max_attempts(7))
                    .timeout_config(timeout_config.clone())
                    .retry_partition(RetryPartition::new("weather-partition")),
            )
            .build()
            .unwrap();
        let (cfg, _) = client.client_config();
        assert_eq!(7, cfg.load::<RetryConfig>().unwrap().max_attempts());
        assert_eq!(&timeout_config, cfg.load::<TimeoutConfig>().unwrap());
        assert_eq!(
            &RetryPartition::new("weather-partition"),
            cfg.load::<RetryPartition>().unwrap()
        );
    }

    #[test]
    fn names_the_retry_partition_after_the_service_by_default() {
        let client = builder().model_file(model("weather.json")).build().unwrap();
        let (cfg, _) = client.client_config();
        assert_eq!(
            &RetryPartition::new("Weather"),
            cfg.load::<RetryPartition>().unwrap()
        );
    }

    #[test]
    fn adds_retry_classifiers_and_interceptors() {
        let client = builder()
            .model_file(model("weather.json"))
            .config(
                Config::builder()
                    .retry_classifier(TestClassifier)
                    .interceptor(NamedInterceptor("first")),
            )
            .build()
            .unwrap();
        let (_, components) = client.client_config();
        assert!(components
            .retry_classifiers()
            .any(|classifier| classifier.name() == "TestClassifier"));
        assert!(components
            .interceptors()
            .any(|interceptor| interceptor.name() == "first"));
    }

    #[test]
    fn a_later_config_appends_interceptors() {
        let client = builder()
            .model_file(model("weather.json"))
            .config(Config::builder().interceptor(NamedInterceptor("first")))
            .config(Config::builder().interceptor(NamedInterceptor("second")))
            .build()
            .unwrap();
        let (_, components) = client.client_config();
        let names: Vec<&str> = components
            .interceptors()
            .map(|interceptor| interceptor.name())
            .filter(|name| ["first", "second"].contains(name))
            .collect();
        assert_eq!(vec!["first", "second"], names);
    }

    #[test]
    fn a_runtime_plugin_overrides_the_config() {
        let client = builder()
            .model_file(model("weather.json"))
            .config(
                Config::builder()
                    .retry_config(RetryConfig::standard().with_max_attempts(7))
                    .runtime_plugin(RetryPlugin(2)),
            )
            .build()
            .unwrap();
        let (cfg, _) = client.client_config();
        assert_eq!(2, cfg.load::<RetryConfig>().unwrap().max_attempts());
    }

    #[derive(Debug)]
    struct CacheRejected;

    impl fmt::Display for CacheRejected {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("the test cache rejected the config")
        }
    }

    impl std::error::Error for CacheRejected {}

    // An identity cache that fails config validation, so a failed build shows the cache is set.
    #[derive(Debug)]
    struct RejectingCache;

    impl ResolveCachedIdentity for RejectingCache {
        fn resolve_cached_identity<'a>(
            &'a self,
            _resolver: SharedIdentityResolver,
            _runtime_components: &'a RuntimeComponents,
            _config_bag: &'a ConfigBag,
        ) -> IdentityFuture<'a> {
            IdentityFuture::ready(Err("unused".into()))
        }

        fn validate_base_client_config(
            &self,
            _runtime_components: &RuntimeComponentsBuilder,
            _cfg: &ConfigBag,
        ) -> Result<(), BoxError> {
            Err(CacheRejected.into())
        }
    }

    #[test]
    fn uses_the_identity_cache_from_the_config() {
        let err = builder()
            .model_file(model("weather.json"))
            .config(Config::builder().identity_cache(RejectingCache))
            .build()
            .unwrap_err();
        assert!(matches!(err.kind(), BuildErrorKind::InvalidConfig { .. }));
        let source = std::error::Error::source(&err).expect("the cache error is the source");
        assert!(source.downcast_ref::<CacheRejected>().is_some());
    }

    #[test]
    fn stores_the_stalled_stream_protection_config() {
        let client = builder()
            .model_file(model("weather.json"))
            .config(
                Config::builder()
                    .stalled_stream_protection(StalledStreamProtectionConfig::disabled()),
            )
            .build()
            .unwrap();
        let (cfg, _) = client.client_config();
        let config = cfg.load::<StalledStreamProtectionConfig>().unwrap();
        assert!(!config.is_enabled());
    }

    // Test keys, defined here as a plugin crate would define its own.
    #[derive(Clone, Debug, PartialEq)]
    struct ReplaceKey(u32);

    impl Storable for ReplaceKey {
        type Storer = StoreReplace<Self>;
    }

    impl ConfigKey for ReplaceKey {
        const APPLIES: &'static [Applies] = &[];
    }

    #[derive(Clone, Debug, PartialEq)]
    struct AppendKey(u32);

    impl Storable for AppendKey {
        type Storer = StoreAppend<Self>;
    }

    impl ConfigKey for AppendKey {
        const APPLIES: &'static [Applies] = &[Applies::EndpointBuiltIn(EndpointBuiltIn::USE_FIPS)];
    }

    #[test]
    fn a_later_put_of_a_replace_key_wins() {
        let client = builder()
            .model_file(model("weather.json"))
            .config(Config::builder().put(ReplaceKey(1)).put(ReplaceKey(2)))
            .config(Config::builder().put(ReplaceKey(3)))
            .build()
            .unwrap();
        let (cfg, _) = client.client_config();
        assert_eq!(Some(&ReplaceKey(3)), cfg.load::<ReplaceKey>());
    }

    #[test]
    fn every_put_of_an_append_key_is_kept() {
        let client = builder()
            .model_file(model("weather.json"))
            .config(Config::builder().put(AppendKey(1)).put(AppendKey(2)))
            .config(Config::builder().put(AppendKey(3)))
            .build()
            .unwrap();
        let (cfg, _) = client.client_config();
        let values: Vec<&AppendKey> = cfg.load::<AppendKey>().collect();
        assert_eq!(vec![&AppendKey(3), &AppendKey(2), &AppendKey(1)], values);
    }

    #[derive(Clone, Debug)]
    struct CborKey;

    impl Storable for CborKey {
        type Storer = StoreReplace<Self>;
    }

    impl ConfigKey for CborKey {
        const APPLIES: &'static [Applies] = &[Applies::Trait(ShapeId::from_static(
            "smithy.protocols#rpcv2Cbor",
        ))];
    }

    #[derive(Clone, Debug)]
    struct WidgetKey;

    impl Storable for WidgetKey {
        type Storer = StoreReplace<Self>;
    }

    impl ConfigKey for WidgetKey {
        const APPLIES: &'static [Applies] = &[Applies::Trait(ShapeId::from_static(
            "example.widgets#widget",
        ))];
    }

    // Returns the keys of `config` that the only service in the model `name` never uses.
    fn unused(name: &str, config: Config) -> Vec<&'static str> {
        let model = Model::from_json_file(model(name)).unwrap();
        let service = model.services().next().unwrap();
        let keys = crate::config::Builder::from(config).puts;
        super::unused_keys(&service, &keys)
    }

    #[test]
    fn a_key_for_a_trait_the_service_has_is_used() {
        let config = Config::builder().put(CborKey).build();
        assert!(unused("weather.json", config).is_empty());
    }

    #[test]
    fn a_key_for_a_trait_the_service_lacks_is_unused() {
        let config = Config::builder().put(WidgetKey).build();
        assert_eq!(
            vec![std::any::type_name::<WidgetKey>()],
            unused("weather.json", config)
        );
    }

    #[test]
    fn a_key_set_twice_is_reported_once() {
        let config = Config::builder().put(WidgetKey).put(WidgetKey).build();
        assert_eq!(1, unused("weather.json", config).len());
    }

    #[test]
    fn only_the_key_for_the_missing_trait_is_reported() {
        let config = Config::builder().put(CborKey).put(WidgetKey).build();
        assert_eq!(
            vec![std::any::type_name::<WidgetKey>()],
            unused("weather.json", config)
        );
    }

    #[test]
    fn a_key_for_a_built_in_the_rules_declare_is_used() {
        let model = Model::from_json_file(model("endpoint-rules.json")).unwrap();
        let service = model.services().next().unwrap();
        let wanted = HashSet::from(["AWS::UseFIPS"]);
        let missing = super::missing_built_ins(&service, wanted).expect("the rules decode");
        assert!(missing.is_empty());
        let config = Config::builder().put(AppendKey(1)).build();
        assert!(unused("endpoint-rules.json", config).is_empty());
    }

    #[test]
    fn a_key_for_a_built_in_is_unused_without_endpoint_rules() {
        let config = Config::builder().put(AppendKey(1)).build();
        assert_eq!(
            vec![std::any::type_name::<AppendKey>()],
            unused("weather.json", config)
        );
    }

    #[test]
    fn a_key_with_no_conditions_is_always_used() {
        let config = Config::builder().put(ReplaceKey(1)).build();
        assert!(unused("weather.json", config).is_empty());
    }

    #[cfg(feature = "aws")]
    mod aws {
        use super::{builder, model};
        use crate::config::{Config, Region, RetryConfig, StalledStreamProtectionConfig};
        use aws_credential_types::Credentials;
        use aws_runtime::invocation_id::{
            InvocationId, InvocationIdGenerator, SharedInvocationIdGenerator,
        };
        use aws_runtime::service_clock_skew::DisableClockSkewCorrection;
        use aws_smithy_runtime_api::box_error::BoxError;
        use aws_types::SdkConfig;

        #[derive(Debug)]
        struct FixedInvocationId;

        impl InvocationIdGenerator for FixedInvocationId {
            fn generate(&self) -> Result<Option<InvocationId>, BoxError> {
                Ok(Some(InvocationId::new("fixed".into())))
            }
        }

        #[test]
        fn stores_the_aws_settings() {
            let client = builder()
                .model_file(model("weather.json"))
                .config(
                    Config::builder()
                        .region(Region::new("us-west-2"))
                        .credentials_provider(Credentials::for_tests())
                        .invocation_id_generator(FixedInvocationId),
                )
                .build()
                .unwrap();
            let (cfg, components) = client.client_config();
            assert_eq!(Some(&Region::new("us-west-2")), cfg.load::<Region>());
            assert!(cfg.load::<SharedInvocationIdGenerator>().is_some());
            assert!(components
                .identity_resolver(&aws_runtime::auth::sigv4::SCHEME_ID)
                .is_some());
        }

        #[test]
        fn stores_the_clock_skew_correction_setting() {
            let client = builder()
                .model_file(model("weather.json"))
                .config(Config::builder().disable_clock_skew_correction(true))
                .build()
                .unwrap();
            let (cfg, _) = client.client_config();
            assert!(cfg
                .load::<DisableClockSkewCorrection>()
                .unwrap()
                .is_disabled());
        }

        #[test]
        fn copies_the_settings_of_an_sdk_config() {
            let sdk_config = SdkConfig::builder()
                .region(Region::new("us-west-2"))
                .retry_config(RetryConfig::standard().with_max_attempts(7))
                .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
                .disable_clock_skew_correction(true)
                .build();
            let client = builder()
                .model_file(model("weather.json"))
                .config(&sdk_config)
                .build()
                .unwrap();
            let (cfg, _) = client.client_config();
            assert_eq!(Some(&Region::new("us-west-2")), cfg.load::<Region>());
            assert_eq!(7, cfg.load::<RetryConfig>().unwrap().max_attempts());
            let stalled_stream_protection = cfg.load::<StalledStreamProtectionConfig>().unwrap();
            assert!(!stalled_stream_protection.is_enabled());
            assert!(cfg
                .load::<DisableClockSkewCorrection>()
                .unwrap()
                .is_disabled());
        }

        #[test]
        fn an_sdk_config_keeps_the_settings_it_leaves_unset() {
            let client = builder()
                .model_file(model("weather.json"))
                .config(Config::builder().region(Region::new("us-west-2")))
                .config(&SdkConfig::builder().build())
                .build()
                .unwrap();
            let (cfg, _) = client.client_config();
            assert_eq!(Some(&Region::new("us-west-2")), cfg.load::<Region>());
        }

        #[test]
        fn a_later_config_region_wins() {
            let client = builder()
                .model_file(model("weather.json"))
                .config(Config::builder().region(Region::new("us-west-2")))
                .config(Config::builder().region(Region::new("eu-west-1")))
                .build()
                .unwrap();
            let (cfg, _) = client.client_config();
            assert_eq!(Some(&Region::new("eu-west-1")), cfg.load::<Region>());
        }
    }
}
