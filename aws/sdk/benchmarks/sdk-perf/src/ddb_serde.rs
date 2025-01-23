/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_dynamodb::operation::put_item::{PutItem, PutItemInput};
use aws_sdk_dynamodb::operation::query::QueryOutput;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_smithy_runtime_api::client::interceptors::context::Input;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_runtime_api::client::ser_de::{DeserializeResponse, SharedResponseDeserializer};
use aws_smithy_runtime_api::client::ser_de::{SerializeRequest, SharedRequestSerializer};
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::ConfigBag;

pub(crate) fn deserialize() {
    use aws_sdk_dynamodb::operation::query::Query;
    use bytes::Bytes;

    let response = HttpResponse::try_from(http::Response::builder()
         .header("server", "Server")
         .header("date", "Mon, 08 Mar 2021 15:51:23 GMT")
         .header("content-type", "application/x-amz-json-1.0")
         .header("content-length", "1231")
         .header("connection", "keep-alive")
         .header("x-amzn-requestid", "A5FGSJ9ET4OKB8183S9M47RQQBVV4KQNSO5AEMVJF66Q9ASUAAJG")
         .header("x-amz-crc32", "624725176")
         .status(http::StatusCode::from_u16(200).unwrap())
         .body(SdkBody::from(Bytes::copy_from_slice(br#"{"Count":2,"Items":[{"year":{"N":"2013"},"info":{"M":{"actors":{"L":[{"S":"Daniel Bruhl"},{"S":"Chris Hemsworth"},{"S":"Olivia Wilde"}]},"plot":{"S":"A re-creation of the merciless 1970s rivalry between Formula One rivals James Hunt and Niki Lauda."},"release_date":{"S":"2013-09-02T00:00:00Z"},"image_url":{"S":"http://ia.media-imdb.com/images/M/MV5BMTQyMDE0MTY0OV5BMl5BanBnXkFtZTcwMjI2OTI0OQ@@._V1_SX400_.jpg"},"genres":{"L":[{"S":"Action"},{"S":"Biography"},{"S":"Drama"},{"S":"Sport"}]},"directors":{"L":[{"S":"Ron Howard"}]},"rating":{"N":"8.3"},"rank":{"N":"2"},"running_time_secs":{"N":"7380"}}},"title":{"S":"Rush"}},{"year":{"N":"2013"},"info":{"M":{"actors":{"L":[{"S":"David Matthewman"},{"S":"Ann Thomas"},{"S":"Jonathan G. Neff"}]},"release_date":{"S":"2013-01-18T00:00:00Z"},"plot":{"S":"A rock band plays their music at high volumes, annoying the neighbors."},"genres":{"L":[{"S":"Comedy"},{"S":"Drama"}]},"image_url":{"S":"http://ia.media-imdb.com/images/N/O9ERWAU7FS797AJ7LU8HN09AMUP908RLlo5JF90EWR7LJKQ7@@._V1_SX400_.jpg"},"directors":{"L":[{"S":"Alice Smith"},{"S":"Bob Jones"}]},"rating":{"N":"6.2"},"rank":{"N":"11"},"running_time_secs":{"N":"5215"}}},"title":{"S":"Turn It Down, Or Else!"}}],"ScannedCount":2}"#)))
         .unwrap()).unwrap();

    let operation = Query::new();
    let config = operation.config().expect("operation should have config");
    let deserializer = config
        .load::<SharedResponseDeserializer>()
        .expect("operation should set a deserializer");

    let output = deserializer
        .deserialize_nonstreaming(&response)
        .expect("success");
    let output = output.downcast::<QueryOutput>().expect("correct type");
    assert_eq!(2, output.count);
}

macro_rules! attr_s {
    ($str_val:expr) => {
        AttributeValue::S($str_val.into())
    };
}
macro_rules! attr_n {
    ($str_val:expr) => {
        AttributeValue::N($str_val.into())
    };
}
macro_rules! attr_list {
    ( $($attr_val:expr),* ) => {
        AttributeValue::L(vec![$($attr_val),*])
    }
}
macro_rules! attr_obj {
    { $($str_val:expr => $attr_val:expr),* } => {
        AttributeValue::M(
            vec![
                $(($str_val.to_string(), $attr_val)),*
            ].into_iter().collect()
        )
    };
}

pub(crate) fn serialize() {
    let input = PutItemInput::builder()
            .table_name("Movies-5")
            .set_item(Some(
                attr_obj! {
                "year" => attr_n!("2013"),
                "title" => attr_s!("Turn It Down, Or Else!"),
                "info" => attr_obj! {
                    "directors" => attr_list![attr_s!("Alice Smith"), attr_s!("Bob Jones")],
                    "release_date" => attr_s!("2013-01-18T00:00:00Z"),
                    "rating" => attr_n!("6.2"),
                    "genres" => attr_list!(attr_s!("Comedy"), attr_s!("Drama")),
                    "image_url" => attr_s!("http://ia.media-imdb.com/images/N/O9ERWAU7FS797AJ7LU8HN09AMUP908RLlo5JF90EWR7LJKQ7@@._V1_SX400_.jpg"),
                    "plot" => attr_s!("A rock band plays their music at high volumes, annoying the neighbors."),
                    "rank" => attr_n!("11"),
                    "running_time_secs" => attr_n!("5215"),
                    "actors" => attr_list!(attr_s!("David Matthewman"), attr_s!("Ann Thomas"), attr_s!("Jonathan G. Neff"))
                }
            }.as_m().unwrap().clone(),
            ))
            .build()
            .expect("valid input");
    let operation = PutItem::new();
    let config = operation.config().expect("operation should have config");
    let serializer = config
        .load::<SharedRequestSerializer>()
        .expect("operation should set a serializer");
    let mut config_bag = ConfigBag::base();
    let input = Input::erase(input.clone());

    let request = serializer
        .serialize_input(input, &mut config_bag)
        .expect("success");
    let body = request.body().bytes().unwrap();
    assert_eq!(body[0], b'{');
}
