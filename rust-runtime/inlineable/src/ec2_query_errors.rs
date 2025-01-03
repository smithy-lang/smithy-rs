/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::error::metadata::{Builder as ErrorMetadataBuilder, ErrorMetadata};
use aws_smithy_xml::decode::{try_data, Document, ScopedDecoder, XmlDecodeError};

#[allow(unused)]
pub fn body_is_error(body: &[u8]) -> Result<bool, XmlDecodeError> {
    let mut doc = Document::try_from(body)?;
    let scoped = doc.root_element()?;
    Ok(scoped.start_el().matches("Response"))
}

pub fn parse_error_metadata(body: &[u8]) -> Result<ErrorMetadataBuilder, XmlDecodeError> {
    let mut doc = Document::try_from(body)?;
    let mut root = doc.root_element()?;
    let mut err_builder = ErrorMetadata::builder();
    while let Some(mut tag) = root.next_tag() {
        if tag.start_el().local() == "Errors" {
            while let Some(mut error_tag) = tag.next_tag() {
                if let "Error" = error_tag.start_el().local() {
                    while let Some(mut error_field) = error_tag.next_tag() {
                        match error_field.start_el().local() {
                            "Code" => {
                                err_builder = err_builder.code(try_data(&mut error_field)?);
                            }
                            "Message" => {
                                err_builder = err_builder.message(try_data(&mut error_field)?);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Ok(err_builder)
}

#[allow(unused)]
pub fn error_scope<'a, 'b>(
    doc: &'a mut Document<'b>,
) -> Result<ScopedDecoder<'b, 'a>, XmlDecodeError> {
    let root = doc
        .next_start_element()
        .ok_or_else(|| XmlDecodeError::custom("no root found searching for an Error"))?;
    if !root.matches("Response") {
        return Err(XmlDecodeError::custom("expected Response as root"));
    }

    while let Some(el) = doc.next_start_element() {
        if el.matches("Errors") && el.depth() == 1 {
            while let Some(el) = doc.next_start_element() {
                if el.matches("Error") && el.depth() == 2 {
                    return Ok(doc.scoped_to(el));
                }
            }
        }
        // otherwise, ignore it
    }
    Err(XmlDecodeError::custom("no error found inside of response"))
}

#[cfg(test)]
mod test {
    use super::{body_is_error, parse_error_metadata};
    use crate::ec2_query_errors::error_scope;
    use aws_smithy_xml::decode::Document;

    #[test]
    fn parse_wrapped_error() {
        let xml = br#"
        <Response>
            <Errors>
                <Error>
                    <Code>InvalidGreeting</Code>
                    <Message>Hi</Message>
                    <AnotherSetting>setting</AnotherSetting>
                    <Ignore><This/></Ignore>
                </Error>
            </Errors>
            <RequestId>foo-id</RequestId>
        </Response>
        "#;
        assert!(body_is_error(xml).unwrap());
        let parsed = parse_error_metadata(xml).expect("valid xml").build();
        assert_eq!(parsed.message(), Some("Hi"));
        assert_eq!(parsed.code(), Some("InvalidGreeting"));
    }

    #[test]
    fn test_error_scope() {
        let xml: &[u8] = br#"
        <Response>
            <RequestId>foo-id</RequestId>
            <MorePreamble>foo-id</RequestId>
            <Sneaky><Error>These are not the errors you are looking for</Error></Sneaky>
            <Errors>
                <Sneaky><Error>These are not the errors you are looking for</Error></Sneaky>
                <Error>
                    <Code>InvalidGreeting</Code>
                    <Message>Hi</Message>
                    <AnotherSetting>setting</AnotherSetting>
                    <Ignore><This/></Ignore>
                </Error>
            </Errors>
            <RequestId>foo-id</RequestId>
        </Response>
        "#;
        let mut doc = Document::try_from(xml).expect("valid");
        let mut error = error_scope(&mut doc).expect("contains error");
        let mut keys = vec![];
        while let Some(tag) = error.next_tag() {
            keys.push(tag.start_el().local().to_owned());
            // read this the full contents of this element
        }
        assert_eq!(keys, vec!["Code", "Message", "AnotherSetting", "Ignore"])
    }
}
