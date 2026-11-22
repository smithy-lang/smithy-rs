/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The dynamic client and its builder.

use crate::call::{operation_plugin, CallBuilder};
use crate::error::{InputError, InputErrorKind, OperationNotFound};
use crate::serde::check_input;
use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
use aws_smithy_lang::shape::ServiceView;
use aws_smithy_lang::ShapeId;
#[cfg(test)]
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_runtime_api::client::runtime_plugin::{RuntimePlugins, SharedRuntimePlugin};
use aws_smithy_schema::serde::SerializableStruct;
use aws_smithy_schema::OperationSchema;
#[cfg(test)]
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::Document;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use struct_value::StructValue;

mod builder;
mod struct_value;

pub(crate) use builder::warn_unused_keys;
pub use builder::DynamicClientBuilder;

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
    pub(crate) schemas: Arc<RuntimeSchemaSet>,
    // The service's operations are sorted by name, so a call finds its operation by binary
    // search.
    pub(crate) operations: Arc<[ShapeId]>,
    // The operation plugin of each operation, at the same index as in `operations`. Each plugin
    // is built on the operation's first call, and later calls share it.
    pub(crate) operation_plugins: Arc<[OnceLock<SharedRuntimePlugin>]>,
    // The service name is the service's SDK ID, or the service's shape name when the model has
    // no SDK ID. Generated clients use the same service name.
    pub(crate) sdk_id: Arc<str>,
    // Whether the service never uses each config key set by `put`, keyed by the key's type.
    // Each key is looked up on its first use, at `build()` or in a call's config override.
    pub(crate) unused_keys: Arc<Mutex<HashMap<TypeId, bool>>>,
    // The default plugins, then the plugin for the client's config, in the order generated
    // clients apply them. The protocol and the service facts are stored in the config plugin's
    // layer.
    pub(crate) runtime_plugins: RuntimePlugins,
}

impl DynamicClient {
    /// Returns a builder for a dynamic client.
    pub fn builder() -> DynamicClientBuilder {
        DynamicClientBuilder::default()
    }

    /// Returns a call to the operation with the shape name `operation`, such as `"GetForecast"`.
    ///
    /// Send the call with [`CallBuilder::send`]. A call to an operation the service does not
    /// have fails when it is sent, and sends no request.
    ///
    /// ```no_run
    /// # async fn example(
    /// #     client: &aws_smithy_dynamic_client::DynamicClient,
    /// # ) -> Result<(), Box<dyn std::error::Error>> {
    /// use aws_smithy_dynamic_client::DynamicOutput;
    /// use aws_smithy_types::Document;
    ///
    /// let input = Document::Object(
    ///     [("city".to_owned(), Document::String("Seattle".to_owned()))].into(),
    /// );
    /// let output: DynamicOutput = client.call("GetForecast").input(input).send().await?;
    /// println!("{:?}", output.members());
    /// # Ok(())
    /// # }
    /// ```
    pub fn call(&self, operation: &str) -> CallBuilder {
        let found = self.operation_index(operation);
        CallBuilder::new(self.clone(), found)
    }

    /// Returns the schemas of the operation with the shape name `operation`, such as
    /// `"GetForecast"`.
    ///
    /// The schemas are the input, the output, and the errors. The errors are the service's
    /// errors, then the operation's own.
    ///
    /// # Errors
    ///
    /// Fails with [`OperationNotFound`] when the service has no operation with that name.
    pub fn operation(&self, operation: &str) -> Result<OperationSchema<'_>, OperationNotFound> {
        let id = &self.operations[self.operation_index(operation)?];
        Ok(self
            .schemas
            .operation(id)
            .expect("the schema set has every operation of the service"))
    }

    /// Returns a value of the structure or union with the shape ID `shape`, built from `value`.
    ///
    /// The value is a document object keyed by the Smithy member names of the shape. The value
    /// implements [`SerializableStruct`], so a protocol or a document type can serialize the value
    /// by the shape's schema.
    ///
    /// ```no_run
    /// # fn example(
    /// #     client: &aws_smithy_dynamic_client::DynamicClient,
    /// # ) -> Result<(), Box<dyn std::error::Error>> {
    /// use aws_smithy_lang::ShapeId;
    /// use aws_smithy_schema::serde::SerializableStruct;
    /// use aws_smithy_types::Document;
    ///
    /// let value = Document::Object(
    ///     [("key".to_owned(), Document::String("color".to_owned()))].into(),
    /// );
    /// let tag = client.struct_value(&ShapeId::new("smithy.example#Tag")?, value)?;
    /// assert_eq!("smithy.example#Tag", tag.schema().shape_id().as_str());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Fails with an [`InputError`] when the service reaches no shape with the ID `shape`, when the
    /// shape is not a structure or a union, or when `value` does not match the shape's schema.
    pub fn struct_value(
        &self,
        shape: &ShapeId,
        value: Document,
    ) -> Result<impl SerializableStruct + Send + Sync + 'static, InputError> {
        let not_found =
            || InputError::for_shape(shape.as_str().to_owned(), InputErrorKind::UnknownShape);
        let schema = self.schemas.schema(shape).ok_or_else(not_found)?;
        if !schema.is_structure() && !schema.is_union() {
            return Err(InputError::for_shape(
                shape.as_str().to_owned(),
                InputErrorKind::NotAStructure,
            ));
        }
        check_input(&self.schemas, schema, &value)?;
        Ok(StructValue::new(value, self.schemas.clone(), shape.clone()))
    }

    // Returns the operation plugin of the operation at `index`, built on the operation's first
    // call.
    pub(crate) fn operation_plugin(&self, index: usize) -> SharedRuntimePlugin {
        self.operation_plugins[index]
            .get_or_init(|| {
                SharedRuntimePlugin::new(operation_plugin(self, &self.operations[index]))
            })
            .clone()
    }

    // Finds the index of an operation of the service by its shape name.
    pub(crate) fn operation_index(&self, operation: &str) -> Result<usize, OperationNotFound> {
        self.operations
            .binary_search_by(|id| id.name().cmp(operation))
            .map_err(|_| OperationNotFound::new(operation.to_owned(), self.service().clone()))
    }

    fn service(&self) -> &ShapeId {
        self.schemas
            .service()
            .expect("the set is built for a service")
    }

    pub(crate) fn service_view(&self) -> ServiceView<'_> {
        self.schemas
            .model()
            .get_shape(self.service())
            .and_then(|shape| shape.as_shape())
            .and_then(|shape| shape.as_service())
            .expect("build() selected a service shape")
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
        cfg.load::<aws_smithy_schema::protocol::SharedClientProtocol>()
            .expect("build() stores the protocol")
            .protocol_id()
            .to_string()
    }
}
