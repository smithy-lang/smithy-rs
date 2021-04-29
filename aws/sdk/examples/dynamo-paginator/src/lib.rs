use async_stream::AsyncStream;

use dynamodb::{
    error::ListTablesError, input::ListTablesInput, output::ListTablesOutput, SdkError,
};

trait PaginateListTables {
    fn paginate(self) -> ListTablesPaginator;
}

impl PaginateListTables for dynamodb::client::fluent_builders::ListTables {
    fn paginate(self) -> ListTablesPaginator {
        let (client, builder) = self.into_inner();
        ListTablesPaginator {
            client,
            inp: builder.build().unwrap(),
        }
    }
}

pub struct ListTablesPaginator {
    client: dynamodb::Client,
    inp: ListTablesInput,
}

impl ListTablesPaginator {
    pub fn send(
        self,
    ) -> impl tokio_stream::Stream<Item = Result<ListTablesOutput, SdkError<ListTablesError>>> {
        let (mut yield_tx, yield_rx) = async_stream::yielder::pair();
        // Without this, customers will get a 500 line error message about pinning. I think
        // one memory allocation for paginating a bunch of results from AWS is worth it.
        Box::pin(AsyncStream::new(yield_rx, async move {
            let mut inp = self.inp;
            loop {
                let rsp = self
                    .client
                    .client()
                    .call(inp.make_operation(self.client.conf()).unwrap())
                    .await;
                match rsp {
                    Ok(output) if output.last_evaluated_table_name.is_some() => {
                        inp.exclusive_start_table_name = output.last_evaluated_table_name.clone();
                        yield_tx.send(Ok(output)).await;
                    }
                    other => {
                        yield_tx.send(other).await;
                        break;
                    }
                }
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::PaginateListTables;
    use aws_hyper::test_connection::TestConnection;
    use dynamodb::{Credentials, Region};
    use smithy_http::body::SdkBody;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn test_paginators() -> Result<(), dynamodb::Error> {
        let conn = TestConnection::new(vec![
            (
                http::Request::new(SdkBody::from("ignore me")),
                http::Response::new(
                    r#"{"TableNames": ["a","b","c"], "LastEvaluatedTableName": "c"}"#,
                ),
            ),
            (
                http::Request::new(SdkBody::from(r#"{"ExclusiveStartTableName": "c"}"#)),
                http::Response::new(r#"{"TableNames": ["d","e"], "LastEvaluatedTableName": null}"#),
            ),
        ]);
        let client = dynamodb::Client::from_conf_conn(
            dynamodb::Config::builder()
                .region(Region::new("us-east-1"))
                .credentials_provider(Credentials::from_keys("akid", "secret", None))
                .build(),
            aws_hyper::conn::Standard::new(conn),
        );

        let mut tables = vec![];
        let mut pages = client.list_tables().paginate().send();
        while let Some(next_page) = pages.try_next().await? {
            tables.extend(next_page.table_names.unwrap_or_default().into_iter());
        }

        assert_eq!(
            tables,
            vec!["a", "b", "c", "d", "e"]
                .iter()
                .map(|k| k.to_string())
                .collect::<Vec<_>>()
        );
        Ok(())
    }
}
