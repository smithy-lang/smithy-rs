#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

git clone https://github.com/awslabs/smithy-rs.git
cd smithy-rs

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