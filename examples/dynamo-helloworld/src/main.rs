/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::error::Error;

use dynamodb::operation::{CreateTable, ListTables};
use dynamodb::{Credentials, Endpoint, Region};
use env_logger::Env;
use dynamodb::model::{KeySchemaElement, KeyType, ProvisionedThroughput, AttributeDefinition, ScalarAttributeType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init_from_env(Env::default().default_filter_or("info"));
    println!("DynamoDB client version: {}", dynamodb::PKG_VERSION);
    let config = dynamodb::Config::builder()
        .region(Region::from("us-east-1"))
        // To load credentials from environment variables, delete this line
        .credentials_provider(Credentials::from_keys(
            "<fill me in2>",
            "<fill me in>",
            None,
        ))
        // To use real DynamoDB, delete this line:
        .endpoint_resolver(Endpoint::immutable(http::Uri::from_static(
            "http://localhost:8000",
        )))
        .build();
    let client = aws_hyper::Client::https();

    let op = ListTables::builder().build(&config);
    // Currently this fails, pending the merge of https://github.com/awslabs/smithy-rs/pull/202
    let tables = client.call(op).await?;
    println!("Current DynamoDB tables: {:?}", tables);

    let new_table = client
        .call(
            CreateTable::builder()
                .table_name("test-table")
                .key_schema(vec![KeySchemaElement::builder().attribute_name("k").key_type(KeyType::Hash).build()])
                .attribute_definitions(vec![AttributeDefinition::builder().attribute_name("k").attribute_type(ScalarAttributeType::S).build()])
                .provisioned_throughput(ProvisionedThroughput::builder().write_capacity_units(10).read_capacity_units(10).build())
                .build(&config),
        )
        .await?;
    println!("new table: {:#?}", &new_table.table_description.unwrap().table_arn.unwrap());
    Ok(())
}
