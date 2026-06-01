/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::orchestrator::Metadata;
use aws_smithy_runtime_api::http::{Request, Response};
use aws_smithy_schema::protocol::{apply_http_endpoint, ClientProtocolInner};
use aws_smithy_schema::serde::{
    SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer,
};
use aws_smithy_schema::{shape_id, Schema, ShapeId};
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::ConfigBag;

use crate::codec::serializer::QueryShapeSerializer;

#[derive(Debug)]
pub struct AwsQueryProtocol {
    protocol_id: ShapeId,
    service_version: String,
}

impl AwsQueryProtocol {
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            protocol_id: shape_id!("aws.protocols", "awsQuery"),
            service_version: version.into(),
        }
    }
}

impl ClientProtocolInner for AwsQueryProtocol {
    type Request = Request;
    type Response = Response;

    fn protocol_id(&self) -> &ShapeId {
        &self.protocol_id
    }

    fn serialize_request(
        &self,
        input: &dyn SerializableStruct,
        input_schema: &Schema,
        endpoint: &str,
        cfg: &ConfigBag,
    ) -> Result<Request, SerdeError> {
        let op_name = cfg
            .load::<Metadata>()
            .map(|m| m.name().to_string())
            .unwrap_or_default();

        let mut serializer = QueryShapeSerializer::new(&op_name, &self.service_version);
        serializer.write_struct(input_schema, input)?;
        let body = aws_smithy_schema::codec::FinishSerializer::finish(serializer);

        let uri = if endpoint.is_empty() { "/" } else { endpoint };
        let mut request = Request::new(SdkBody::from(body));
        request
            .set_method("POST")
            .map_err(|e| SerdeError::custom(format!("{e}")))?;
        request
            .set_uri(uri)
            .map_err(|e| SerdeError::custom(format!("{e}")))?;
        request
            .headers_mut()
            .insert("Content-Type", "application/x-www-form-urlencoded");
        if let Some(len) = request.body().content_length() {
            request
                .headers_mut()
                .insert("Content-Length", len.to_string());
        }
        Ok(request)
    }

    fn deserialize_response<'a>(
        &self,
        response: &'a Response,
        _output_schema: &Schema,
        _cfg: &ConfigBag,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
        let body = response
            .body()
            .bytes()
            .ok_or_else(|| SerdeError::custom("response body not available"))?;
        let body_str = std::str::from_utf8(body).map_err(|e| SerdeError::InvalidInput {
            message: e.to_string(),
        })?;
        if body_str.trim().is_empty() {
            return Ok(Box::new(
                aws_smithy_xml::codec::XmlShapeDeserializer::from_str(
                    "",
                    1,
                    aws_smithy_xml::codec::DEFAULT_MAX_DEPTH,
                ),
            ));
        }
        let inner = strip_aws_query_envelope(body_str)?;
        // Start at depth=1 so read_struct won't try to strip a root element —
        // the envelope has already been stripped.
        Ok(Box::new(
            aws_smithy_xml::codec::XmlShapeDeserializer::from_str(
                inner,
                1,
                aws_smithy_xml::codec::DEFAULT_MAX_DEPTH,
            ),
        ))
    }

    fn update_endpoint(
        &self,
        request: &mut Request,
        endpoint: &aws_smithy_types::endpoint::Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), SerdeError> {
        apply_http_endpoint(request, endpoint, cfg)
    }
}

fn strip_aws_query_envelope(xml: &str) -> Result<&str, SerdeError> {
    use xmlparser::{ElementEnd, Token, Tokenizer};
    let mut tokenizer = Tokenizer::from(xml);
    let mut depth: u32 = 0;
    let mut result_content_start: usize = 0;
    let mut result_content_end: usize = 0;
    let mut root_content_start: usize = 0;
    let mut root_content_end: usize = 0;
    let mut found_target = false;

    while let Some(Ok(token)) = tokenizer.next() {
        match token {
            Token::ElementStart { local, .. } => {
                depth += 1;
                if depth == 2 && !found_target {
                    let name = local.as_str();
                    if name.ends_with("Result") || name == "Error" {
                        found_target = true;
                    }
                }
            }
            Token::ElementEnd { end, span } => match end {
                ElementEnd::Open => {
                    if depth == 1 {
                        root_content_start = span.end();
                    } else if depth == 2 && found_target {
                        result_content_start = span.end();
                    }
                }
                ElementEnd::Close(_, _) => {
                    if depth == 2 && found_target {
                        result_content_end = span.start();
                        break;
                    }
                    if depth == 1 {
                        root_content_end = span.start();
                    }
                    depth -= 1;
                }
                ElementEnd::Empty => {
                    depth -= 1;
                }
            },
            _ => {}
        }
    }
    if found_target && result_content_start > 0 {
        Ok(&xml[result_content_start..result_content_end])
    } else if root_content_start > 0 {
        Ok(&xml[root_content_start..root_content_end])
    } else {
        Ok("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::protocol::ClientProtocolInner;
    use aws_smithy_schema::serde::ShapeSerializer;
    use aws_smithy_schema::ShapeType;
    use aws_smithy_types::config_bag::Layer;

    struct EmptyInput;
    impl SerializableStruct for EmptyInput {
        fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            Ok(())
        }
    }

    static SCHEMA: Schema = Schema::new(shape_id!("test", "Input"), ShapeType::Structure);

    fn cfg_with_metadata() -> ConfigBag {
        let mut layer = Layer::new("test");
        layer.store_put(Metadata::new("GetUser", "MyService"));
        ConfigBag::of_layers(vec![layer])
    }

    #[test]
    fn request_has_correct_content_type() {
        let cfg = cfg_with_metadata();
        let request = AwsQueryProtocol::new("2012-11-05")
            .serialize_request(&EmptyInput, &SCHEMA, "https://example.com", &cfg)
            .unwrap();
        assert_eq!(
            request.headers().get("Content-Type").unwrap(),
            "application/x-www-form-urlencoded"
        );
    }

    #[test]
    fn request_has_action_and_version() {
        let cfg = cfg_with_metadata();
        let request = AwsQueryProtocol::new("2012-11-05")
            .serialize_request(&EmptyInput, &SCHEMA, "https://example.com", &cfg)
            .unwrap();
        let body = std::str::from_utf8(request.body().bytes().unwrap()).unwrap();
        assert!(body.contains("Action=GetUser"));
        assert!(body.contains("Version=2012-11-05"));
    }

    #[test]
    fn request_posts_to_endpoint() {
        let cfg = cfg_with_metadata();
        let request = AwsQueryProtocol::new("1.0")
            .serialize_request(
                &EmptyInput,
                &SCHEMA,
                "https://sqs.us-east-1.amazonaws.com",
                &cfg,
            )
            .unwrap();
        assert_eq!(request.uri(), "https://sqs.us-east-1.amazonaws.com");
    }

    #[test]
    fn request_defaults_to_slash() {
        let cfg = cfg_with_metadata();
        let request = AwsQueryProtocol::new("1.0")
            .serialize_request(&EmptyInput, &SCHEMA, "", &cfg)
            .unwrap();
        assert_eq!(request.uri(), "/");
    }

    #[test]
    fn deserialize_response_strips_envelope() {
        let xml = "<GetUserResponse><GetUserResult><Name>Alice</Name><Age>30</Age></GetUserResult></GetUserResponse>";
        let response = Response::new(200u16.try_into().unwrap(), SdkBody::from(xml));

        static NAME: Schema = Schema::new_member(shape_id!("t", "S"), ShapeType::String, "Name", 0);
        static AGE: Schema = Schema::new_member(shape_id!("t", "S"), ShapeType::Integer, "Age", 1);
        static OUT_SCHEMA: Schema =
            Schema::new_struct(shape_id!("t", "S"), ShapeType::Structure, &[&NAME, &AGE]);

        let mut deser = AwsQueryProtocol::new("1.0")
            .deserialize_response(&response, &OUT_SCHEMA, &ConfigBag::base())
            .unwrap();
        let mut name = String::new();
        let mut age = 0i32;
        deser
            .read_struct(&OUT_SCHEMA, &mut |member, d| {
                match member.member_name() {
                    Some("Name") => name = d.read_string(member)?,
                    Some("Age") => age = d.read_integer(member)?,
                    _ => {}
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(name, "Alice");
        assert_eq!(age, 30);
    }

    #[test]
    fn protocol_id() {
        assert_eq!(
            AwsQueryProtocol::new("1.0").protocol_id().as_str(),
            "aws.protocols#awsQuery"
        );
    }
}
