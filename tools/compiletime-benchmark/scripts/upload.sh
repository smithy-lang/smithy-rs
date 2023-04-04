# TODO
aws s3 cp ./unoptimized.txt $S3_BUCKET_TARGET &
aws s3 cp ./optimized.txt $S3_BUCKET_TARGET &
wait