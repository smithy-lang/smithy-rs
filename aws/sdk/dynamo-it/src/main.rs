/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::error::Error;

use dynamodb::output::{DeleteTableOutput, ListTablesOutput};
use operation::{SdkBody, Operation, ParseStrictResponse, Request};
use dynamodb::error::DeleteTableError;

struct DeleteTable(dynamodb::operation::DeleteTable);

//use bytes::Bytes;
//use auth::{SigningConfig, ServiceConfig, RequestConfig};
//use std::time::SystemTime;
//use operation::endpoint::StaticEndpoint;
use http::{Response, Uri};
use dynamodb::operation::CreateTable;
use dynamodb::model::{AttributeDefinition, ScalarAttributeType, KeySchemaElement, ProvisionedThroughput, KeyType};
use bytes::Bytes;
use auth::{RequestConfig, ServiceConfig};
use std::time::SystemTime;


impl ParseStrictResponse for DeleteTable {
    type Output = Result<DeleteTableOutput, DeleteTableError>;
    fn parse(&self, response: &Response<Bytes>) -> Self::Output {
        self.0.parse_response(response)
    }
}

use auth::SigningConfig;

impl DeleteTable {
    fn into_operation(self, config: dynamodb::Config) -> Operation<DeleteTable> {
        let mut request = operation::Request::new(self.0.build_http_request().map(|body|SdkBody::from(body)));
        request.extensions.insert(SigningConfig::default_configuration(
            RequestConfig {
                request_ts: ||SystemTime::now()
            },
            ServiceConfig {
                service: "dynamodb".into(),
                region: "us-east-1".into()
            }
        ));
        request.extensions.insert(config.credentials_provider);
        Operation {
            request,
            response_handler: Box::new(self)
        }
    }
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let table_name = "new_table";
    //let client = aws_hyper::Client::default();
    let client = io_v0::Client::local("dynamodb");
    let config = dynamodb::Config::builder().build();
    let clear_table = dynamodb::operation::DeleteTable::builder()
        .table_name(table_name)
        .build(&config);


    match io_v0::dispatch!(client, clear_table).parsed() {
        Ok(Ok(table_deleted)) => println!(
            "{:?} was deleted",
            table_deleted
                .table_description
                .as_ref()
                .unwrap()
                .table_name
                .as_ref()
                .unwrap()
        ),
        Ok(Err(table_del_error)) => println!("failed to delete table: {}", table_del_error),
        Err(e) => println!("dispatch error: {:?}", e),
    }

    let tables = io_v0::dispatch!(
        client,
        dynamodb::operation::ListTables::builder().build(&config)
    )
    .parsed
    .unwrap();
    assert_eq!(
        tables.unwrap(),
        ListTablesOutput::builder().table_names(vec![]).build()
    );
    println!("no tables...creating table");

    let create_table = CreateTable::builder()
        .table_name(table_name)
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

    let response = io_v0::dispatch!(client, create_table);
    match response.parsed {
        Some(Ok(output)) => {
            assert_eq!(
                output.table_description.unwrap().table_name.unwrap(),
                table_name
            );
            println!("{} was created", table_name);
        }
        _ => println!("{:?}", response.raw),
    }

    let tables = io_v0::dispatch!(
        client,
        dynamodb::operation::ListTables::builder().build(&config)
    )
    .parsed
    .unwrap();
    println!(
        "current tables: {:?}",
        &tables.as_ref().unwrap().table_names
    );
    assert_eq!(
        tables.unwrap(),
        ListTablesOutput::builder()
            .table_names(vec![table_name.to_string()])
            .build()
    );

    Ok(())
}
