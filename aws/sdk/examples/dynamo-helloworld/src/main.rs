/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::error::Error;

use aws_hyper::{SdkError, SdkSuccess};
use dynamodb::error::ListTablesError;
use dynamodb::output::ListTablesOutput;
use dynamodb::{
    model::{
        AttributeDefinition, KeySchemaElement, KeyType, ProvisionedThroughput, ScalarAttributeType,
    },
    operation::CreateTable,
};
use env_logger::Env;
use operationwip::endpoint::StaticEndpoint;
use operationwip::retry_policy::{RetryPolicy, RetryType};
use tokio::time::Duration;

#[derive(Clone)]
struct RetryIfNoTables;
impl RetryPolicy<SdkSuccess<ListTablesOutput>, SdkError<ListTablesError>> for RetryIfNoTables {
    fn should_retry(
        &self,
        input: Result<&SdkSuccess<ListTablesOutput>, &SdkError<ListTablesError>>,
    ) -> Option<RetryType> {
        match input {
            Ok(list_tables) => {
                if list_tables
                    .parsed
                    .table_names
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or_default()
                    == 0
                {
                    Some(RetryType::Explicit(Duration::new(5, 0)))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init_from_env(Env::default().default_filter_or("info"));
    let config = dynamodb::Config::builder()
        .region("us-east-1")
        // To load credentials from environment variables, delete this line
        .credentials_provider(auth::Credentials::from_static(
            "<fill me in2>",
            "<fill me in>",
        ))
        // To use real DynamoDB, delete this line:
        .endpoint_provider(StaticEndpoint::from_uri(http::Uri::from_static(
            "http://localhost:8000",
        )))
        .build();
    let client = aws_hyper::Client::default().with_tracing();
    let list_tables = dynamodb::operation::ListTables::builder()
        .build(&config)
        .with_policy(RetryIfNoTables);

    let response = client.call(list_tables).await;
    let tables = match response {
        Ok(output) => output.table_names.unwrap(),
        Err(e) => panic!("err: {:?}", e),
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
            Ok(created) => println!("table created! {:#?}", created),
            Err(failed) => println!("failed to create table: {:?}", failed),
        }
    }

    let list_tables = dynamodb::operation::ListTables::builder().build(&config);

    let response = client.call(list_tables).await;
    match response {
        Ok(output) => {
            println!(
                "tables: {:?}",
                output.table_names.unwrap_or_default()
            );
        }
        Err(e) => panic!("err: {:?}", e.error()),
    };

    Ok(())
}
