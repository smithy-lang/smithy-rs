/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::error::Error;

use dynamodb::{model::{AttributeDefinition, KeySchemaElement, KeyType, ProvisionedThroughput, ScalarAttributeType}, operation::CreateTable};
use http::Uri;
use operation::endpoint::StaticEndpoint;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = dynamodb::Config::builder()
        .region("us-east-1")
        .endpoint_provider(StaticEndpoint::from_uri(Uri::from_static(
            "http://localhost:8000",
        )))
        .build();
    let client = aws_hyper::Client::default();
    let list_tables = dynamodb::operation::ListTables::builder().build(&config);

    let response = client.call(list_tables).await;
    let tables = match response {
        Ok(output) => {
            output.parsed.table_names.unwrap()
        },
        Err(e) => panic!("err: {:?}", e.error()),
    };
    if tables.is_empty() {
        let create_table = CreateTable::builder()
        .table_name("new_table")
        .attribute_definitions(vec![AttributeDefinition::builder()
            .attribute_name("ForumName")
            .attribute_type(ScalarAttributeType::S)
            .build()])
        .key_schema(vec![KeySchemaElement::builder()
            .attribute_name("ForumName")
            .key_type(KeyType::Hash)
            .build()])
        .provisioned_throughput(
            ProvisionedThroughput::builder()
                .read_capacity_units(100)
                .write_capacity_units(100)
                .build(),
        )
        .build(&config);
        match client.call(create_table).await {
            Ok(created) => println!("table created! {:#?}", created.parsed),
            Err(failed) => println!("failed to create table: {:?}", failed)
        }
    }

    let list_tables = dynamodb::operation::ListTables::builder().build(&config);

    let response = client.call(list_tables).await;
    match response {
        Ok(output) => {
            println!("tables: {:?}", output.parsed.table_names.unwrap_or_default());
        },
        Err(e) => panic!("err: {:?}", e.error()),
    };

    Ok(())
}
