#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

git clone https://github.com/awslabs/smithy-rs.git
cd smithy-rs
git checkout $TARGET_COMMIT_HASH

./gradlew :aws:sdk:cargoCheck
WORKDIR=pwd
PATH_TO_GENERATED_SDK="$WORKDIR/aws/sdk/build/aws-sdk/sdk"
export RUSTFLAGS="--cfg aws-sdk-unstable"

for i in $(ls $PATH_TO_GENERATED_SDK); do
    cd $PATH_TO_GENERATED_SDK/$i
    if [[ ! $($PATH_TO_GENERATED_SDK == *"aws-"*) ]]; then
        # not-optimized
        echo sdk $i >> unoptimized.txt
        time cargo build >> unoptimized.txt
        echo "=======================================" >> unoptimized.txt

        # optimized
        echo sdk $i >> optimized.txt
        time cargo build --release >> optimized.txt
        echo "=======================================" >> optimized.txt
    fi
done

python3 script.py

# uploads the results to some where...
# I think we should just upload them to github or something but I will pretend that it uploads to a S3 bucket
S3_BUCKETT="s3://my-bucket/$S3_KEY/$COMMIT_HASH/"
aws s3 cp ./unoptimized.md $S3_BUCKETT &
aws s3 cp ./optimized.md $S3_BUCKETT &
aws s3 cp ./unoptimized.csv $S3_BUCKETT &
aws s3 cp ./optimized.csv $S3_BUCKETT &
wait