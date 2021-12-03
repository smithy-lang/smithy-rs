#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

HEAD_COMMIT_SHA=$(git rev-parse HEAD)
cd $(git rev-parse --show-toplevel)/target/doc

{
    echo '<!doctype html>'
    echo '<html>'
    echo '<head>'
    echo '  <metadata charset="utf-8">'
    echo "  <title>Doc preview for ${HEAD_COMMIT_SHA}</title>"
    echo '</head>'
    echo '<body>'
    echo "  <h1>Doc preview for ${HEAD_COMMIT_SHA}</h1>"
} > index.html

doc_link () {
    local package=$1
    if [[ -d ${package} ]]; then
        echo "    <li><a href='${package}/index.html'>${package}</a></li>" >> index.html
    fi
}

echo '  <h3>AWS Services</h3><ul>' >> index.html
for package in $(ls | grep aws_sdk_ | sort); do doc_link ${package}; done
echo '  </ul>' >> index.html

echo '  <h3>AWS Runtime Crates</h3><ul>' >> index.html
for package in $(ls | grep aws_ | grep -v _smithy_ | sort); do doc_link ${package}; done
echo '  </ul>' >> index.html

echo '  <h3>Smithy Runtime Crates</h3><ul>' >> index.html
for package in $(ls | grep aws_smithy_ | sort); do doc_link ${package}; done
echo '  </ul>' >> index.html

{
    echo '</body>'
    echo '</html>'
} >> index.html
