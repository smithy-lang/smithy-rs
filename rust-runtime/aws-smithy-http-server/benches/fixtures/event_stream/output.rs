/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! One-time snapshot of generated event-stream benchmark SDK code. See README.md.
#![allow(dead_code)]

#[allow(missing_docs)] // documentation missing in model
#[derive(::std::fmt::Debug)]
pub struct DownloadOutput {
    #[allow(missing_docs)] // documentation missing in model
    pub metadata: ::std::string::String,
    #[allow(missing_docs)] // documentation missing in model
    pub events: ::aws_smithy_http::event_stream::EventStreamSender<crate::model::Events, crate::error::EventsError>,
}
impl DownloadOutput {
    #[allow(missing_docs)] // documentation missing in model
    pub fn metadata(&self) -> &str {
        use std::ops::Deref;
        self.metadata.deref()
    }
    #[allow(missing_docs)] // documentation missing in model
    pub fn events(
        &self,
    ) -> &::aws_smithy_http::event_stream::EventStreamSender<crate::model::Events, crate::error::EventsError> {
        &self.events
    }
}
static DOWNLOADOUTPUT_MEMBER_METADATA_TRAITS: std::sync::LazyLock<::aws_smithy_schema::TraitMap> =
    std::sync::LazyLock::new(|| {
        let mut map = ::aws_smithy_schema::TraitMap::new();
        map.insert(::std::boxed::Box::new(::aws_smithy_schema::AnnotationTrait::new(
            ::aws_smithy_schema::ShapeId::from_parts("smithy.api#required", "smithy.api", "required"),
        )));
        map
    });
static DOWNLOADOUTPUT_MEMBER_METADATA: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StreamOutput$metadata",
        "benchmark.eventstream",
        "StreamOutput",
    ),
    ::aws_smithy_schema::ShapeType::String,
    "metadata",
    0,
)
.with_http_header("x-benchmark-metadata")
.with_traits(&DOWNLOADOUTPUT_MEMBER_METADATA_TRAITS);
static DOWNLOADOUTPUT_MEMBER_EVENTS: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StreamOutput$events",
        "benchmark.eventstream",
        "StreamOutput",
    ),
    ::aws_smithy_schema::ShapeType::Union,
    "events",
    1,
)
.with_http_payload()
.with_streaming();
static DOWNLOADOUTPUT_SCHEMA: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_struct(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StreamOutput",
        "benchmark.eventstream",
        "StreamOutput",
    ),
    ::aws_smithy_schema::ShapeType::Structure,
    &[&DOWNLOADOUTPUT_MEMBER_METADATA, &DOWNLOADOUTPUT_MEMBER_EVENTS],
)
.with_http(aws_smithy_schema::traits::HttpTrait::new("POST", "/download", None))
.with_original_name("StreamOutput");
impl crate::output::DownloadOutput {
    /// The schema for this shape.
    pub const SCHEMA: &'static ::aws_smithy_schema::Schema<'static> = &DOWNLOADOUTPUT_SCHEMA;
}
impl ::aws_smithy_schema::serde::SerializableStruct for crate::output::DownloadOutput {
    fn schema(&self) -> &::aws_smithy_schema::Schema<'_> {
        Self::SCHEMA
    }

    #[allow(unused_variables, clippy::diverging_sub_expression)]
    fn serialize_members(
        &self,
        ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        {
            let val = &self.metadata;
            ser.write_string(&DOWNLOADOUTPUT_MEMBER_METADATA, val)?;
        }
        Ok(())
    }
}
