// Setting names borrowed from Boto3's transfer.py
// https://github.com/boto/boto3/blob/096e45841545f24bbd572a62504cc0dbb62e6b07/boto3/s3/transfer.py#L299-L307

#[derive(Default, Debug)]
pub(crate) struct Settings {
    pub(crate) multipart_threshold: usize,
    pub(crate) max_concurrency: usize,
    pub(crate) multipart_chunksize: usize,
    pub(crate) num_download_attempts: usize,
    pub(crate) max_io_queue: usize,
    pub(crate) io_chunksize: usize,
}