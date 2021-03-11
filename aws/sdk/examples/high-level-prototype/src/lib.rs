pub mod hl;

#[cfg(test)]
mod tests {
    use dynamodb::model::{AttributeDefinition, ScalarAttributeType, KeySchemaElement, KeyType, ProvisionedThroughput};
    use crate::hl::DynamoDb;

    #[tokio::test]
    async fn list_tables() {
        let client = DynamoDb::from_env();
        let new_table = client.create_table()
            .table_name("my-fancy-new-table")
            .key_schema(vec![
                KeySchemaElement::builder()
                    .attribute_name("year")
                    .key_type(KeyType::Hash)
                    .build(),
                KeySchemaElement::builder()
                    .attribute_name("title")
                    .key_type(KeyType::Range)
                    .build(),
            ])
            .attribute_definitions(vec![
                AttributeDefinition::builder()
                    .attribute_name("year")
                    .attribute_type(ScalarAttributeType::N)
                    .build(),
                AttributeDefinition::builder()
                    .attribute_name("title")
                    .attribute_type(ScalarAttributeType::S)
                    .build(),
            ])
            .provisioned_throughput(
                ProvisionedThroughput::builder()
                    .read_capacity_units(10)
                    .write_capacity_units(10)
                    .build(),
            ).execute().await;

        eprintln!("Created a new table: {:?}", new_table);
        let tables = client
            .list_tables()
            .limit(10)
            .exclusive_start_table_name("first_table")
            .execute()
            .await;
        eprintln!("{:#?}", tables);
    }
}
