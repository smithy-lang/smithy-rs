/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::error::Error;

use dynamodb::error::DeleteTableError;
use dynamodb::output::{DeleteTableOutput, ListTablesOutput};

use dynamodb::model::{
    AttributeDefinition, KeySchemaElement, KeyType, ProvisionedThroughput, ScalarAttributeType,
};
use dynamodb::operation::CreateTable;
use http::Uri;
use operation::endpoint::StaticEndpoint;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let table_name = "new_table";
    let config = dynamodb::Config::builder()
        .region("us-east-1")
        .endpoint_provider(StaticEndpoint::from_uri(Uri::from_static(
            "http://localhost:8000",
        )))
        .build();
    let client = aws_hyper::Client::default();
    let delete_table = dynamodb::operation::DeleteTable::builder()
        .table_name(table_name)
        .build(&config);
    let response = client.call(delete_table).await;
    match response {
        Ok(output) => println!("deleted! {:?}", output.parsed),
        Err(e) => println!("err: {:?}", e.error()),
    }
    Ok(())
}
