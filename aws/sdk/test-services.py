#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

"""
Generate a list of services which have non-trivial unit tests

This script generates output like `-p aws-sdk-s3 -p aws-sdk-dynamodb`. It is intended to be used in conjunction with
`cargo test` to compile a subset of services when running tests:

```bash
cargo test $(python test-services.py)
```
"""

import os
from pathlib import Path


def main():
    # change working directory to `aws/sdk`:
    script_path = os.path.abspath(__file__)
    os.chdir(os.path.dirname(script_path))

    services = set()
    for service in os.listdir('integration-tests'):
        if os.path.isdir(Path('integration-tests') / service):
            services.add(service)

    for model in os.listdir('aws-models'):
        if model.endswith('-tests.smithy'):
            service = model[:-len('-tests.smithy')]
            services.add(service)

    services = sorted(list(services))
    aws_packages = set()
    for aws_package in os.listdir('../rust-runtime'):
        if aws_package.startswith('aws-') and 'inlineable' not in aws_package:
            aws_packages.add(aws_package)
    as_arguments = [f'-p aws-sdk-{service}' for service in services]
    as_arguments.extend([f'-p {aws_package}' for aws_package in aws_packages])

    print(' '.join(as_arguments), end='')


if __name__ == "__main__":
    main()
