/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod rejection;
pub mod router;
pub mod runtime_error;

/// [AWS restXml](https://smithy.io/2.0/aws/protocols/aws-restxml-protocol.html) protocol.
#[derive(Debug, Default, Clone, Copy)]
pub struct RestXml;

/// Stateful schema-driven restXml protocol implementation.
#[derive(Debug)]
pub struct RestXmlProtocol {
    pub(crate) inner: crate::schema::protocol::rest::RestProtocol<aws_smithy_xml::codec::XmlCodec>,
}

impl Default for RestXmlProtocol {
    fn default() -> Self {
        Self {
            inner: crate::schema::protocol::rest::RestProtocol::new(
                aws_smithy_xml::codec::XmlCodec::new(aws_smithy_xml::codec::XmlCodecSettings::default()),
                crate::schema::protocol::rest_xml::POLICY,
            ),
        }
    }
}
