#!/usr/bin/env bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
# Regenerates expected/ with the pinned Smithy Java rules engine. See README.md.
set -euo pipefail

VERSION=1.73.0
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../../../../.." && pwd)"
JARS="${SMITHY_JARS:-$HOME/.gradle/caches/modules-2/files-2.1/software.amazon.smithy}"

jar() {
    local found
    found=$(ls "$JARS"/"$1"/"$VERSION"/*/"$1-$VERSION$2.jar" 2>/dev/null | head -1)
    [ -n "$found" ] || { echo "missing $1-$VERSION$2.jar under $JARS" >&2; exit 1; }
    echo "$found"
}

CP=""
for artifact in smithy-model smithy-utils smithy-jmespath smithy-build smithy-rules-engine \
    smithy-aws-endpoints smithy-aws-traits smithy-model-jmespath smithy-rules-engine-tests; do
    CP="$CP${CP:+:}$(jar "$artifact" "")"
done

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
# java-fixes.patch applies to the pinned sources; compile it ahead of the jars.
(cd "$WORK" && unzip -q "$(jar smithy-rules-engine -sources)" \
    'software/amazon/smithy/rulesengine/logic/cfg/CoalesceTransform.java' \
    'software/amazon/smithy/rulesengine/logic/cfg/VariableConsolidationTransform.java' \
    'software/amazon/smithy/rulesengine/traits/EndpointBddTrait.java' \
    && patch -s -p1 < "$HERE/java-fixes.patch" \
    && javac -nowarn -d classes -cp "$CP" $(find software -name '*.java'))

quiet() { grep -v '^INFO:\|^[A-Z][a-z][a-z] [0-9]\|^FINE' || true; }
java -cp "$CP" "$HERE/Generate.java" "$HERE" "$REPO/aws/sdk/aws-models" 2>&1 | quiet
java -cp "$WORK/classes:$CP" "$HERE/Generate.java" "$HERE" "$REPO/aws/sdk/aws-models" --patched 2>&1 | quiet
