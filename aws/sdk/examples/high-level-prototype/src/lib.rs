use aws_hyper::conn::Standard;
use aws_hyper::SdkError;
use dynamodb::error::{ListTablesError, CreateTableError};
use dynamodb::input::{list_tables_input, create_table_input};

use dynamodb::output::{ListTablesOutput, CreateTableOutput};
use dynamodb::Config;

use std::sync::Arc;
use dynamodb::model::{AttributeDefinition, KeySchemaElement, LocalSecondaryIndex, GlobalSecondaryIndex, BillingMode, ProvisionedThroughput, StreamSpecification, SSESpecification, Tag};

struct Handle {
    conn: aws_hyper::Client<Standard>,
    conf: dynamodb::Config,
}

pub struct DynamoDb {
    handle: Arc<Handle>
}

pub struct ListTablesFluentBuilder {
    inner: list_tables_input::Builder,
    ddb: Arc<Handle>
}


impl ListTablesFluentBuilder {
    fn new(handler: Arc<Handle>) -> Self {
        ListTablesFluentBuilder {
            ddb: handler,
            inner: Default::default(),
        }
    }
    /// <p>The first table name that this operation will evaluate. Use the value that was returned for
    /// <code>LastEvaluatedTableName</code> in a previous operation, so that you can obtain the next page
    /// of results.</p>
    pub fn exclusive_start_table_name(mut self, inp: impl Into<::std::string::String>) -> Self {
        self.inner = self.inner.exclusive_start_table_name(inp);
        self
    }
    /// <p>A maximum number of table names to return. If this parameter is not specified, the limit is 100.</p>
    pub fn limit(mut self, inp: i32) -> Self {
        self.inner = self.inner.limit(inp);
        self
    }

    pub async fn execute(self) -> Result<ListTablesOutput, SdkError<ListTablesError>> {
        let op = self.inner.build(&self.ddb.conf);
        self.ddb.conn.call(op).await
    }
}

pub struct CreateTableFluentBuilder {
    inner: create_table_input::Builder,
    ddb: Arc<Handle>
}

impl CreateTableFluentBuilder {
    fn new(handler: Arc<Handle>) -> Self {
        CreateTableFluentBuilder {
            ddb: handler,
            inner: Default::default(),
        }
    }

    /// <p>An array of attributes that describe the key schema for the table and indexes.</p>
    pub fn attribute_definitions(mut self, inp: ::std::vec::Vec<AttributeDefinition>) -> Self {
        self.inner = self.inner.attribute_definitions(inp);
        self
    }

    /// <p>The name of the table to create.</p>
    pub fn table_name(mut self, inp: impl Into<::std::string::String>) -> Self {
        self.inner = self.inner.table_name(inp);
        self
    }

    /// <p>Specifies the attributes that make up the primary key for a table or an index. The attributes
    /// in <code>KeySchema</code> must also be defined in the <code>AttributeDefinitions</code> array. For more
    /// information, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/DataModel.html">Data Model</a> in the
    /// <i>Amazon DynamoDB Developer Guide</i>.</p>
    /// <p>Each <code>KeySchemaElement</code> in the array is composed of:</p>
    /// <ul>
    /// <li>
    /// <p>
    /// <code>AttributeName</code> - The name of this key attribute.</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>KeyType</code> - The role that the key attribute will assume:</p>
    /// <ul>
    /// <li>
    /// <p>
    /// <code>HASH</code> - partition key</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>RANGE</code> - sort key</p>
    /// </li>
    /// </ul>
    /// </li>
    /// </ul>
    /// <note>
    /// <p>The partition key of an item is also known as its <i>hash
    /// attribute</i>. The term "hash attribute" derives from the DynamoDB usage of
    /// an internal hash function to evenly distribute data items across partitions, based
    /// on their partition key values.</p>
    /// <p>The sort key of an item is also known as its <i>range attribute</i>.
    /// The term "range attribute" derives from the way DynamoDB stores items with the same
    /// partition key physically close together, in sorted order by the sort key value.</p>
    /// </note>
    /// <p>For a simple primary key (partition key), you must provide
    /// exactly one element with a <code>KeyType</code> of <code>HASH</code>.</p>
    /// <p>For a composite primary key (partition key and sort key), you must provide exactly two
    /// elements, in this order: The first element must have a <code>KeyType</code> of <code>HASH</code>,
    /// and the second element must have a <code>KeyType</code> of <code>RANGE</code>.</p>
    /// <p>For more information, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/WorkingWithTables.html#WorkingWithTables.primary.key">Working with Tables</a> in the <i>Amazon DynamoDB Developer
    /// Guide</i>.</p>
    pub fn key_schema(mut self, inp: ::std::vec::Vec<KeySchemaElement>) -> Self {
        self.inner = self.inner.key_schema(inp);
        self
    }
    /// <p>One or more local secondary indexes (the maximum is 5) to be created on the table. Each index is scoped to a given partition key value. There is a 10 GB size limit per partition key value; otherwise, the size of a local secondary index is unconstrained.</p>
    /// <p>Each local secondary index in the array includes the following:</p>
    /// <ul>
    /// <li>
    /// <p>
    /// <code>IndexName</code> - The name of the local secondary index. Must be unique only for this table.</p>
    /// <p></p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>KeySchema</code> - Specifies the key schema for the local secondary index. The key schema must begin with
    /// the same partition key as the table.</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>Projection</code> - Specifies
    /// attributes that are copied (projected) from the table into the index. These are in
    /// addition to the primary key attributes and index key
    /// attributes, which are automatically projected. Each
    /// attribute specification is composed of:</p>
    /// <ul>
    /// <li>
    /// <p>
    /// <code>ProjectionType</code> - One
    /// of the following:</p>
    /// <ul>
    /// <li>
    /// <p>
    /// <code>KEYS_ONLY</code> - Only the index and primary keys are projected into the
    /// index.</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>INCLUDE</code> - Only the specified table attributes are
    /// projected into the index. The list of projected attributes is in
    /// <code>NonKeyAttributes</code>.</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>ALL</code> - All of the table attributes are projected into the
    /// index.</p>
    /// </li>
    /// </ul>
    /// </li>
    /// <li>
    /// <p>
    /// <code>NonKeyAttributes</code> - A list of one or more non-key
    /// attribute names that are projected into the secondary index. The total
    /// count of attributes provided in <code>NonKeyAttributes</code>,
    /// summed across all of the secondary indexes, must not exceed 100. If you
    /// project the same attribute into two different indexes, this counts as
    /// two distinct attributes when determining the total.</p>
    /// </li>
    /// </ul>
    /// </li>
    /// </ul>
    pub fn local_secondary_indexes(
        mut self,
        inp: ::std::vec::Vec<LocalSecondaryIndex>,
    ) -> Self {
        self.inner = self.inner.local_secondary_indexes(inp);
        self
    }
    /// <p>One or more global secondary indexes (the maximum is 20) to be created on the table. Each global secondary index in the array includes the following:</p>
    /// <ul>
    /// <li>
    /// <p>
    /// <code>IndexName</code> - The name of the global secondary index. Must be unique only for this table.</p>
    /// <p></p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>KeySchema</code> - Specifies the key schema for the global secondary index.</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>Projection</code> - Specifies
    /// attributes that are copied (projected) from the table into the index. These are in
    /// addition to the primary key attributes and index key
    /// attributes, which are automatically projected. Each
    /// attribute specification is composed of:</p>
    /// <ul>
    /// <li>
    /// <p>
    /// <code>ProjectionType</code> - One
    /// of the following:</p>
    /// <ul>
    /// <li>
    /// <p>
    /// <code>KEYS_ONLY</code> - Only the index and primary keys are projected into the
    /// index.</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>INCLUDE</code> - Only the specified table attributes are
    /// projected into the index. The list of projected attributes is in
    /// <code>NonKeyAttributes</code>.</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>ALL</code> - All of the table attributes are projected into the
    /// index.</p>
    /// </li>
    /// </ul>
    /// </li>
    /// <li>
    /// <p>
    /// <code>NonKeyAttributes</code> - A list of one or more non-key attribute names that are
    /// projected into the secondary index. The total count of attributes provided in <code>NonKeyAttributes</code>, summed across all of the secondary indexes, must not exceed 100. If you project the same attribute into two different indexes, this counts as two distinct attributes when determining the total.</p>
    /// </li>
    /// </ul>
    /// </li>
    /// <li>
    /// <p>
    /// <code>ProvisionedThroughput</code> - The provisioned throughput settings for the global secondary index,
    /// consisting of read and write capacity units.</p>
    /// </li>
    /// </ul>
    pub fn global_secondary_indexes(
        mut self,
        inp: ::std::vec::Vec<GlobalSecondaryIndex>,
    ) -> Self {
        self.inner = self.inner.global_secondary_indexes(inp);
        self
    }
    /// <p>Controls how you are charged for read and write throughput and how you manage capacity. This setting can be changed later.</p>
    /// <ul>
    /// <li>
    /// <p>
    /// <code>PROVISIONED</code> - We recommend using <code>PROVISIONED</code> for predictable workloads. <code>PROVISIONED</code> sets the billing mode to <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/HowItWorks.ReadWriteCapacityMode.html#HowItWorks.ProvisionedThroughput.Manual">Provisioned Mode</a>.</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>PAY_PER_REQUEST</code> - We recommend using <code>PAY_PER_REQUEST</code> for unpredictable workloads. <code>PAY_PER_REQUEST</code> sets the billing mode to <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/HowItWorks.ReadWriteCapacityMode.html#HowItWorks.OnDemand">On-Demand Mode</a>.
    /// </p>
    /// </li>
    /// </ul>
    pub fn billing_mode(mut self, inp: BillingMode) -> Self {
        self.inner = self.inner.billing_mode(inp);
        self
    }
    /// <p>Represents the provisioned throughput settings for a specified table or index. The
    /// settings can be modified using the <code>UpdateTable</code> operation.</p>
    /// <p> If you set BillingMode as <code>PROVISIONED</code>, you must specify this property. If you
    /// set BillingMode as <code>PAY_PER_REQUEST</code>, you cannot specify this
    /// property.</p>
    /// <p>For current minimum and maximum provisioned throughput values, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Limits.html">Service,
    /// Account, and Table Quotas</a> in the <i>Amazon DynamoDB Developer
    /// Guide</i>.</p>
    pub fn provisioned_throughput(mut self, inp: ProvisionedThroughput) -> Self {
        self.inner = self.inner.provisioned_throughput(inp);
        self
    }
    /// <p>The settings for DynamoDB Streams on the table. These settings consist of:</p>
    /// <ul>
    /// <li>
    /// <p>
    /// <code>StreamEnabled</code> - Indicates whether DynamoDB Streams is to be enabled
    /// (true) or disabled (false).</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>StreamViewType</code> - When an item in the table is modified, <code>StreamViewType</code>
    /// determines what information is written to the table's stream. Valid values for
    /// <code>StreamViewType</code> are:</p>
    /// <ul>
    /// <li>
    /// <p>
    /// <code>KEYS_ONLY</code> - Only the key attributes of the modified item are written to the
    /// stream.</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>NEW_IMAGE</code> - The entire item, as it appears after it was modified, is written
    /// to the stream.</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>OLD_IMAGE</code> - The entire item, as it appeared before it was modified, is
    /// written to the stream.</p>
    /// </li>
    /// <li>
    /// <p>
    /// <code>NEW_AND_OLD_IMAGES</code> - Both the new and the old item images of the item are
    /// written to the stream.</p>
    /// </li>
    /// </ul>
    /// </li>
    /// </ul>
    pub fn stream_specification(mut self, inp: StreamSpecification) -> Self {
        self.inner = self.inner.stream_specification(inp);
        self
    }
    /// <p>Represents the settings used to enable server-side encryption.</p>
    pub fn sse_specification(mut self, inp: SSESpecification) -> Self {
        self.inner = self.inner.sse_specification(inp);
        self
    }
    /// <p>A list of key-value pairs to label the table. For more information, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Tagging.html">Tagging for DynamoDB</a>.</p>
    pub fn tags(mut self, inp: ::std::vec::Vec<Tag>) -> Self {
        self.inner = self.inner.tags(inp);
        self
    }

    pub async fn execute(self) -> Result<CreateTableOutput, SdkError<CreateTableError>> {
        let op = self.inner.build(&self.ddb.conf);
        self.ddb.conn.call(op).await
    }
}

impl DynamoDb {
    pub fn list_tables(&self) -> ListTablesFluentBuilder {
        ListTablesFluentBuilder::new(self.handle.clone())
    }

    pub fn create_table(&self) -> CreateTableFluentBuilder {
        CreateTableFluentBuilder::new(self.handle.clone())
    }

    pub fn from_env() -> Self {
        DynamoDb {
            handle: Arc::new(Handle{
                conf: Config::builder().build(),
                conn: aws_hyper::Client::https()
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::DynamoDb;
    use dynamodb::model::{AttributeDefinition, ScalarAttributeType, KeySchemaElement, KeyType, ProvisionedThroughput};

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
