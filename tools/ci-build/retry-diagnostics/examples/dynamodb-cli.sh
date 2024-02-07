#!/bin/bash

# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

export AWS_ACCESS_KEY_ID="1234"
export AWS_SECRET_ACCESS_KEY="1234"


# Initial ID value
id=1

# Infinite loop
while true; do
  # Run AWS CLI get-item command
  aws dynamodb get-item --table-name ExampleTable --key "{\"Id\": {\"S\": \"$id\"}}" --endpoint-url http://localhost:3000 --cli-read-timeout 1

  # Increment ID
  ((id++))

  # Optional: sleep for a second to avoid overwhelming the database
  sleep 1
done
