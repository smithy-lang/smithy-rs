/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_hyper::test_connection::TestConnection;
use aws_sdk_dynamodb as dynamodb;
use aws_sdk_dynamodb::input::{PutItemInput, QueryInput};
use aws_sdk_dynamodb::model::AttributeValue;
use criterion::{criterion_group, criterion_main, Criterion};
use dynamodb::{Credentials, Region};
use http::Uri;
use smithy_http::body::SdkBody;
use std::collections::HashMap;
use tokio::runtime::Builder as RuntimeBuilder;

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

fn put_item() -> PutItemInput {
    PutItemInput::builder()
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
            }
            .as_m()
            .unwrap()
            .clone(),
        ))
        .build()
        .expect("valid operation")
}

fn query() -> QueryInput {
    let mut expr_attrib_names = HashMap::new();
    expr_attrib_names.insert("#yr".to_string(), "year".to_string());
    let mut expr_attrib_values = HashMap::new();
    expr_attrib_values.insert(":yyyy".to_string(), attr_n!("2013"));
    QueryInput::builder()
        .table_name("Movies-5")
        .key_condition_expression("#yr = :yyyy")
        .set_expression_attribute_names(Some(expr_attrib_names))
        .set_expression_attribute_values(Some(expr_attrib_values))
        .build()
        .expect("valid operation")
}

async fn do_bench() {
    let conn = test_connection();
    let client = aws_hyper::Client::new(conn.clone());
    let conf = dynamodb::Config::builder()
        .region(Region::new("us-east-1"))
        .credentials_provider(Credentials::from_keys("AKNOTREAL", "NOT_A_SECRET", None))
        .build();

    client
        .call(put_item().make_operation(&conf).expect("valid request"))
        .await
        .unwrap();

    let query_result = client
        .call(query().make_operation(&conf).expect("valid request"))
        .await
        .unwrap();
    assert_eq!(2, query_result.count);
}

fn bench_group(c: &mut Criterion) {
    let runtime = RuntimeBuilder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    c.bench_function("ser_deser_bench", |b| {
        b.to_async(&runtime).iter(|| do_bench())
    });
}

criterion_group!(benches, bench_group);
criterion_main!(benches);

fn test_connection() -> TestConnection<&'static str> {
    let flows = vec![
        (http::Request::builder()
        .header("content-type", "application/x-amz-json-1.0")
        .header("x-amz-target", "DynamoDB_20120810.PutItem")
        .header("content-length", "619")
        .header("host", "dynamodb.us-east-1.amazonaws.com")
        .header("authorization", "AWS4-HMAC-SHA256 Credential=ASIAR6OFQKMAFQIIYZ5T/20210308/us-east-1/dynamodb/aws4_request, SignedHeaders=content-length;content-type;host;x-amz-target, Signature=85fc7d2064a0e6d9c38d64751d39d311ad415ae4079ef21ef254b23ecf093519")
        .header("x-amz-date", "20210308T155123Z")
        .uri(Uri::from_static("https://dynamodb.us-east-1.amazonaws.com/"))
        .body(SdkBody::from(r#"{"TableName":"Movies-5","Item":{"info":{"M":{"rating":{"N":"6.2"},"genres":{"L":[{"S":"Comedy"},{"S":"Drama"}]},"image_url":{"S":"http://ia.media-imdb.com/images/N/O9ERWAU7FS797AJ7LU8HN09AMUP908RLlo5JF90EWR7LJKQ7@@._V1_SX400_.jpg"},"release_date":{"S":"2013-01-18T00:00:00Z"},"actors":{"L":[{"S":"David Matthewman"},{"S":"Ann Thomas"},{"S":"Jonathan G. Neff"}]},"plot":{"S":"A rock band plays their music at high volumes, annoying the neighbors."},"running_time_secs":{"N":"5215"},"rank":{"N":"11"},"directors":{"L":[{"S":"Alice Smith"},{"S":"Bob Jones"}]}}},"title":{"S":"Turn It Down, Or Else!"},"year":{"N":"2013"}}}"#)).unwrap(),
        http::Response::builder()
            .header("server", "Server")
            .header("date", "Mon, 08 Mar 2021 15:51:23 GMT")
            .header("content-type", "application/x-amz-json-1.0")
            .header("content-length", "2")
            .header("connection", "keep-alive")
            .header(
                "x-amzn-requestid",
                "E6TGS5HKHHV08HSQA31IO1IDMFVV4KQNSO5AEMVJF66Q9ASUAAJG",
            )
            .header("x-amz-crc32", "2745614147")
            .status(http::StatusCode::from_u16(200).unwrap())
            .body(r#"{}"#)
            .unwrap()
    ),
    (http::Request::builder()
         .header("content-type", "application/x-amz-json-1.0")
         .header("x-amz-target", "DynamoDB_20120810.Query")
         .header("content-length", "156")
         .header("host", "dynamodb.us-east-1.amazonaws.com")
         .header("authorization", "AWS4-HMAC-SHA256 Credential=ASIAR6OFQKMAFQIIYZ5T/20210308/us-east-1/dynamodb/aws4_request, SignedHeaders=content-length;content-type;host;x-amz-target, Signature=504d6b4de7093b20255b55057085937ec515f62f3c61da68c03bff3f0ce8a160")
         .header("x-amz-date", "20210308T155123Z")
         .uri(Uri::from_static("https://dynamodb.us-east-1.amazonaws.com/"))
         .body(SdkBody::from(r##"{"TableName":"Movies-5","KeyConditionExpression":"#yr = :yyyy","ExpressionAttributeNames":{"#yr":"year"},"ExpressionAttributeValues":{":yyyy":{"N":"2013"}}}"##)).unwrap(),
    http::Response::builder()
         .header("server", "Server")
         .header("date", "Mon, 08 Mar 2021 15:51:23 GMT")
         .header("content-type", "application/x-amz-json-1.0")
         .header("content-length", "1231")
         .header("connection", "keep-alive")
         .header("x-amzn-requestid", "A5FGSJ9ET4OKB8183S9M47RQQBVV4KQNSO5AEMVJF66Q9ASUAAJG")
         .header("x-amz-crc32", "624725176")
         .status(http::StatusCode::from_u16(200).unwrap())
         .body(r#"{"Count":2,"Items":[{"year":{"N":"2013"},"info":{"M":{"actors":{"L":[{"S":"Daniel Bruhl"},{"S":"Chris Hemsworth"},{"S":"Olivia Wilde"}]},"plot":{"S":"A re-creation of the merciless 1970s rivalry between Formula One rivals James Hunt and Niki Lauda."},"release_date":{"S":"2013-09-02T00:00:00Z"},"image_url":{"S":"http://ia.media-imdb.com/images/M/MV5BMTQyMDE0MTY0OV5BMl5BanBnXkFtZTcwMjI2OTI0OQ@@._V1_SX400_.jpg"},"genres":{"L":[{"S":"Action"},{"S":"Biography"},{"S":"Drama"},{"S":"Sport"}]},"directors":{"L":[{"S":"Ron Howard"}]},"rating":{"N":"8.3"},"rank":{"N":"2"},"running_time_secs":{"N":"7380"}}},"title":{"S":"Rush"}},{"year":{"N":"2013"},"info":{"M":{"actors":{"L":[{"S":"David Matthewman"},{"S":"Ann Thomas"},{"S":"Jonathan G. Neff"}]},"release_date":{"S":"2013-01-18T00:00:00Z"},"plot":{"S":"A rock band plays their music at high volumes, annoying the neighbors."},"genres":{"L":[{"S":"Comedy"},{"S":"Drama"}]},"image_url":{"S":"http://ia.media-imdb.com/images/N/O9ERWAU7FS797AJ7LU8HN09AMUP908RLlo5JF90EWR7LJKQ7@@._V1_SX400_.jpg"},"directors":{"L":[{"S":"Alice Smith"},{"S":"Bob Jones"}]},"rating":{"N":"6.2"},"rank":{"N":"11"},"running_time_secs":{"N":"5215"}}},"title":{"S":"Turn It Down, Or Else!"}}],"ScannedCount":2}"#).unwrap()
    )];
    TestConnection::new(flows)
}
