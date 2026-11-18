// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Direct event marshalling and unmarshalling without HTTP or CRC framing.
use crate::{Case, Direction, Fixture, Protocol};
use aws_smithy_eventstream::frame::{MarshallMessage, UnmarshallMessage, UnmarshalledMessage};
use aws_smithy_http_server::schema::{
    protocol::{RestJson1, RpcV2Cbor},
    SharedServerProtocol,
};
use aws_smithy_types::event_stream::Message;
use std::fmt::Debug;

/// Prepared inputs and the exact generated SDK's serde implementations.
pub struct SerdeFixture<T, M, U> {
    pub events: Vec<T>,
    pub messages: Vec<Message>,
    pub marshaller: M,
    pub unmarshaller: U,
    case: Case,
}

impl<T, M, U> SerdeFixture<T, M, U>
where
    T: Clone + Debug + PartialEq,
    M: MarshallMessage<Input = T>,
    U: UnmarshallMessage<Output = T>,
    U::Error: Debug,
{
    /// Checks independent fixtures and round trips, and primes reusable codec caches.
    /// This happens before Criterion starts timing.
    pub fn validate(&self) {
        assert_eq!(self.events.len(), self.case.count);
        assert_eq!(self.messages.len(), self.case.count);
        let fixture = Fixture::new(self.case);
        for (sequence, (event, message)) in self.events.iter().zip(&self.messages).enumerate() {
            fixture.validate_event(sequence, message);
            let encoded = self
                .marshaller
                .marshall(event.clone())
                .expect("serialize event");
            fixture.validate_event(sequence, &encoded);
            for input in [message, &encoded] {
                match self
                    .unmarshaller
                    .unmarshall(input)
                    .expect("deserialize event")
                {
                    UnmarshalledMessage::Event(decoded) => assert_eq!(&decoded, event),
                    UnmarshalledMessage::Error(error) => {
                        panic!("unexpected modeled error: {error:?}")
                    }
                }
            }
        }
    }
}

fn protocol(protocol: Protocol) -> SharedServerProtocol {
    match protocol {
        Protocol::RestJson => SharedServerProtocol::metadata_routed(RestJson1::default()),
        Protocol::RpcV2Cbor => SharedServerProtocol::metadata_routed(RpcV2Cbor::default()),
    }
}

macro_rules! constructor {
    ($sdk:ident, $name:ident, legacy, $protocol:expr) => {
        ::$sdk::benchmark_serde::$name::new()
    };
    ($sdk:ident, $name:ident, schema, $protocol:expr) => {
        ::$sdk::benchmark_serde::$name::new(protocol($protocol))
    };
}

// Use concrete generated types; no trait-object dispatch surrounds the serde call.
macro_rules! sdk_fixture {
    ($module:ident, $sdk:ident, $serde:ident) => {
        pub mod $module {
            use super::*;

            pub fn fixture(
                case: Case,
            ) -> SerdeFixture<
                ::$sdk::model::Events,
                ::$sdk::benchmark_serde::EventsMarshaller,
                ::$sdk::benchmark_serde::EventsUnmarshaller,
            > {
                assert_eq!(case.direction, Direction::Upload);
                SerdeFixture {
                    events: (0..case.count)
                        .map(|sequence| crate::services::$module::event(case, sequence))
                        .collect(),
                    messages: Fixture::new(case).event_messages(),
                    marshaller: constructor!($sdk, EventsMarshaller, $serde, case.protocol),
                    unmarshaller: constructor!($sdk, EventsUnmarshaller, $serde, case.protocol),
                    case,
                }
            }
        }
    };
}

sdk_fixture!(rest_json_legacy, rest_json_legacy, legacy);
sdk_fixture!(rest_json_schema, rest_json_schema, schema);
sdk_fixture!(cbor_legacy, cbor_legacy, legacy);
sdk_fixture!(cbor_schema, cbor_schema, schema);
sdk_fixture!(multi_rest_json_schema, multi_rest_json_schema, schema);
sdk_fixture!(multi_schema, multi_schema, schema);
