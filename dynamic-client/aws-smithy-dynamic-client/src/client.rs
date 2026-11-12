/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The dynamic client and its builder.

use crate::config;
use crate::error::{BuildError, MissingHttpClient};
use crate::protocol::{detect, protocol_traits, AWS_QUERY};
use aws_smithy_lang::runtime_schema::{Materialization, RuntimeSchemaSet};
use aws_smithy_lang::shape::ServiceView;
use aws_smithy_lang::{Model, ShapeId};
use aws_smithy_runtime::client::defaults::{default_plugins, DefaultPluginParams};
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
use aws_smithy_runtime_api::client::runtime_components::{
    RuntimeComponentsBuilder, SharedConfigValidator,
};
use aws_smithy_runtime_api::client::runtime_plugin::{RuntimePlugins, StaticRuntimePlugin};
use aws_smithy_schema::protocol::SharedClientProtocol;
use aws_smithy_types::config_bag::{ConfigBag, Layer};
use std::path::PathBuf;
use std::sync::Arc;

/// A client for a Smithy service, built from a model loaded at runtime.
///
/// Create one with [`DynamicClient::builder`].
///
/// ```no_run
/// use aws_smithy_dynamic_client::config::{BehaviorVersion, Config};
/// use aws_smithy_dynamic_client::DynamicClient;
///
/// let client = DynamicClient::builder()
///     .model_file("weather.json")
///     .config(Config::builder().behavior_version(BehaviorVersion::latest()))
///     .build()?;
/// # Ok::<(), aws_smithy_dynamic_client::BuildError>(())
/// ```
#[derive(Clone, Debug)]
pub struct DynamicClient {
    // The schemas of the selected service, built on first use. The set also holds the model
    // and the service ID.
    // TODO(dynamic-client): Remove this `allow` once calls read the schemas.
    #[allow(dead_code)]
    schemas: Arc<RuntimeSchemaSet>,
    // The default plugins, then the plugin for the client's config, in the order generated
    // clients apply them. The protocol is stored in the config plugin's layer.
    // TODO(dynamic-client): Remove this `allow` once calls read the runtime plugins.
    #[allow(dead_code)]
    runtime_plugins: RuntimePlugins,
}

impl DynamicClient {
    /// Returns a builder for a dynamic client.
    pub fn builder() -> Builder {
        Builder::default()
    }

    #[cfg(test)]
    fn service(&self) -> &ShapeId {
        self.schemas
            .service()
            .expect("the set is built for a service")
    }

    #[cfg(test)]
    fn client_config(&self) -> (ConfigBag, RuntimeComponentsBuilder) {
        let mut cfg = ConfigBag::base();
        let components = self
            .runtime_plugins
            .apply_client_configuration(&mut cfg)
            .expect("build() applied the same plugins");
        (cfg, components)
    }

    #[cfg(test)]
    fn protocol_id(&self) -> String {
        let (cfg, _) = self.client_config();
        cfg.load::<SharedClientProtocol>()
            .expect("build() stores the protocol")
            .protocol_id()
            .to_string()
    }
}

/// A builder for a [`DynamicClient`].
#[derive(Clone, Debug, Default)]
pub struct Builder {
    model_file: Option<PathBuf>,
    service: Option<ShapeId>,
    config: config::Builder,
}

impl Builder {
    /// Sets the path of the Smithy JSON AST file to load the model from.
    ///
    /// [`build`](Builder::build) reads the file.
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
            None => detect(&service_view).ok_or_else(|| {
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
        components.push_config_validator(SharedConfigValidator::base_client_config_fn(
            validate_http_client,
        ));
        let runtime_plugins =
            runtime_plugins(components, behavior_version, service.name(), protocol);
        // The same check generated clients run when they are created.
        let mut cfg = ConfigBag::base();
        let components = runtime_plugins
            .apply_client_configuration(&mut cfg)
            .map_err(BuildError::invalid_config)?;
        components
            .validate_base_client_config(&cfg)
            .map_err(BuildError::invalid_config)?;
        Ok(DynamicClient {
            schemas: Arc::new(schemas),
            runtime_plugins,
        })
    }
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

// The default plugins, then a plugin for the client's config, as generated clients build them.
fn runtime_plugins(
    components: RuntimeComponentsBuilder,
    behavior_version: BehaviorVersion,
    service_name: &str,
    protocol: SharedClientProtocol,
) -> RuntimePlugins {
    let mut layer = Layer::new("DynamicClientConfig");
    layer.store_put(protocol);
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
    use super::DynamicClient;
    use crate::config::{BehaviorVersion, Config};
    use crate::error::BuildErrorKind;
    #[cfg(not(feature = "default-https-client"))]
    use crate::error::MissingHttpClient;
    use crate::BuildError;
    use aws_smithy_async::test_util::instant_time_and_sleep;
    use aws_smithy_http_client::test_util::NeverClient;
    use aws_smithy_lang::runtime_schema::Materialization;
    use aws_smithy_lang::ShapeId;
    use std::path::PathBuf;
    use std::time::UNIX_EPOCH;

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
    fn builder() -> super::Builder {
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

    fn protocols(service: &str) -> super::Builder {
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
}
