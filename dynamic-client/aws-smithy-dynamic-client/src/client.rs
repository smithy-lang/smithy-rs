/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The dynamic client and its builder.

use crate::error::BuildError;
use aws_smithy_lang::shape::ServiceView;
use aws_smithy_lang::{Model, ShapeId};
use std::path::PathBuf;

/// A client for a Smithy service, built from a model loaded at runtime.
///
/// Create one with [`DynamicClient::builder`].
///
/// ```no_run
/// use aws_smithy_dynamic_client::DynamicClient;
///
/// let client = DynamicClient::builder()
///     .model_file("weather.json")
///     .build()?;
/// # Ok::<(), aws_smithy_dynamic_client::BuildError>(())
/// ```
#[derive(Clone, Debug)]
// TODO(dynamic-client): Remove this `allow` once calls read the model and the service.
#[allow(dead_code)]
pub struct DynamicClient {
    model: Model,
    service: ShapeId,
}

impl DynamicClient {
    /// Returns a builder for a dynamic client.
    pub fn builder() -> Builder {
        Builder::default()
    }
}

/// A builder for a [`DynamicClient`].
#[derive(Clone, Debug, Default)]
pub struct Builder {
    model_file: Option<PathBuf>,
    service: Option<ShapeId>,
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

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// Fails when the builder has no model, or when the model file fails to load. Also fails
    /// when the model has no service, or has several and none is specified. Also fails when
    /// the model has no service with the given ID.
    pub fn build(self) -> Result<DynamicClient, BuildError> {
        let path = self.model_file.ok_or_else(BuildError::missing_model)?;
        let model = Model::from_json_file(&path)
            .map_err(|source| BuildError::model_load_failed(path, source))?;
        let service_view = select_service(&model, self.service)?;
        let service = service_view.id().clone();
        Ok(DynamicClient { model, service })
    }
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
    use crate::error::BuildErrorKind;
    use crate::BuildError;
    use aws_smithy_lang::ShapeId;
    use std::path::PathBuf;

    fn model(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/models")
            .join(name)
    }

    fn id(id: &str) -> ShapeId {
        ShapeId::new(id).unwrap()
    }

    fn build_error(file: &str, service: Option<&str>) -> BuildError {
        let mut builder = DynamicClient::builder().model_file(model(file));
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
        let client = DynamicClient::builder()
            .model_file(model("weather.json"))
            .build()
            .unwrap();
        assert_eq!("example.weather#Weather", client.service.as_str());
    }

    #[test]
    fn selects_the_specified_service() {
        let client = DynamicClient::builder()
            .model_file(model("two-services.json"))
            .service(id("example.cafe#Cafe"))
            .build()
            .unwrap();
        assert_eq!("example.cafe#Cafe", client.service.as_str());
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
    fn fails_on_a_service_id_of_another_shape() {
        let err = build_error("no-service.json", Some("example.weather#Forecast"));
        assert!(matches!(
            err.kind(),
            BuildErrorKind::UnknownService { id: unknown, services }
                if *unknown == id("example.weather#Forecast") && services.is_empty()
        ));
    }
}
