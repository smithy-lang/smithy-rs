/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::error::Error;

use dynamo::model::{
    AttributeDefinition, KeySchemaElement, KeyType, ProvisionedThroughput, ScalarAttributeType,
};
use dynamo::operation::CreateTable;
use dynamo::output::ListTablesOutput;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let table_name = "new_table";
    let client = io_v0::Client::local("dynamodb");
    let config = dynamo::Config::from_env();
    let clear_table = dynamo::operation::DeleteTable::builder()
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
        dynamo::operation::ListTables::builder().build(&config)
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
            .attribute_type(ScalarAttributeType::from("S"))
            .build()])
        .key_schema(vec![KeySchemaElement::builder()
            .attribute_name("ForumName")
            .key_type(KeyType::from("HASH"))
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
        dynamo::operation::ListTables::builder().build(&config)
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
