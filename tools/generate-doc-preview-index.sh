#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 <base commit hash>"
    exit 1
fi

BASE_COMMIT_SHA=$1
HEAD_COMMIT_SHA=$(git rev-parse HEAD)
DOC_TITLE_CONTEXT=$(git log -1 --oneline)
cd $(git rev-parse --show-toplevel)/target/doc

CHANGED_RUNTIME_CRATES_FILE=$(mktemp)
git diff --name-only $BASE_COMMIT_SHA -- $(git rev-parse --show-toplevel)/rust-runtime | cut -d '/' -f2 | sed 's/-/_/g' | sort | uniq > "${CHANGED_RUNTIME_CRATES_FILE}"

{
    echo '<!doctype html>'
    echo '<html>'
    echo '<head>'
    echo '  <metadata charset="utf-8">'
    echo "  <title>Doc preview: ${DOC_TITLE_CONTEXT}</title>"
    echo '</head>'
    echo '<body>'
    echo "  <h2>Doc preview: ${DOC_TITLE_CONTEXT}</h2>"
} > index.html

doc_link () {
    local crate=$1
    if [[ -d ${crate} ]]; then
        grep -q -e "^${crate}$" "${CHANGED_RUNTIME_CRATES_FILE}"
        CHANGED=$?
        if [[ $CHANGED -eq 0 ]]; then
          echo "  <li><strong><a href='${crate}/index.html'>${crate}</a></strong></li>" >> index.html
        else
          echo "  <li><a href='${crate}/index.html'>${crate}</a></li>" >> index.html
        fi
    fi
}

echo '  <h3>AWS Services</h3><ul>' >> index.html
for crate in $(ls -d aws_sdk_*); do doc_link ${crate}; done
echo '  </ul>' >> index.html

echo '  <h3>AWS Runtime Crates</h3><ul>' >> index.html
for crate in $(ls -d aws_* | grep -v '_smithy_\|_sdk_'); do doc_link ${crate}; done
echo '  </ul>' >> index.html

echo '  <h3>Smithy Runtime Crates</h3><ul>' >> index.html
for crate in $(ls -d aws_smithy_*); do doc_link ${crate}; done
echo '  </ul>' >> index.html

{
    echo "<p><strong>Bold</strong> indicates a runtime crate had changes since the base revision \
         $(git log -1 --oneline ${BASE_COMMIT_SHA}). These may or may not be documentation changes.</p>"
    echo '</body>'
    echo '</html>'
} >> index.html

rm "${CHANGED_RUNTIME_CRATES_FILE}"
