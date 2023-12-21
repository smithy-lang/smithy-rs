#!/usr/bin/env python3
#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

import sys
import os
import os.path as path
from pathlib import Path
import subprocess

# Ensure working directory is the script path
script_path = path.dirname(path.realpath(__file__))

# Looks for aws-sdk-rust in the parent directory of smithy-rs
def discover_aws_models():
    repo_path = path.abspath(path.join(script_path, "../../../aws-sdk-rust"))
    git_path = repo_path + "/.git"
    if path.exists(repo_path) and path.exists(git_path):
        print(f"Discovered aws-models at {repo_path}")
        return Path(repo_path) / 'aws-models'
    else:
        return None

def copy_model(source_path, model_path):
    dest_path = Path("aws-models") / model_path
    source = source_path.read_text()
    # Add a newline at the end when copying the model over
    with open(dest_path, "w") as file:
        file.write(source)
        if not source.endswith("\n"):
            file.write("\n")

def copy_known_models(aws_models_repo):
    known_models = set()
    for model_file in os.listdir("aws-models"):
        if not model_file.endswith('.json'):
            continue
        known_models.add(model_file)
        source_path = Path(aws_models_repo) / model_file
        if not source_path.exists():
            print(f"  Warning: cannot find model for '{model_file}' in aws-models, but it exists in smithy-rs")
            continue
        copy_model(source_path, model_file)
    return known_models

def copy_sdk_configs(aws_models_repo):
    for model_file in os.listdir(aws_models_repo):
        if model_file.startswith('sdk-'):
            print('copying SDK configuration file', model_file)
            copy_model(aws_models_repo / model_file, model_file)
def main():
    # Acquire model location
    aws_models_repo = discover_aws_models()
    if aws_models_repo == None:
        if len(sys.argv) != 2:
            print("Please provide the location of the aws-models repository as the first argument")
            sys.exit(1)
        else:
            aws_models_repo = sys.argv[1]

    print("Copying over known models...")
    copy_known_models(aws_models_repo)
    copy_sdk_configs(aws_models_repo)

    print("Models synced.")

if __name__ == "__main__":
    main()
