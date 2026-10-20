#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

# Probes for the Coral Java sample server (the one with the /stringlist echo operation), to learn how
# Coral answers the restXml requests where the schema serde and generated serde smithy-rs servers differ.
#
#   bash coral-xml-probes.sh                 # against http://localhost:8000
#   BASE=http://host:8000 bash coral-xml-probes.sh > coral-results.txt
#
# Each probe prints its id, the request, what the two smithy-rs servers answer, and Coral's full
# response (status line, headers, body). The smithy-rs results were measured with the equivalent
# request against `POST /greet/{name}` (body member `tags`, a list of strings), where the XML content
# type is `application/xml`; wherever a probe sends $XML_CT, smithy-rs was sent `application/xml`.
#
# Adjust these if the sample service differs.
BASE="${BASE:-http://localhost:8000}"
OP_PATH="${OP_PATH:-/stringlist}"          # the list echo operation
ROOT="${ROOT:-StringListInput}"            # its input root element
NS="${NS:-http://internal.amazon.com/coral/com.amazon.coraltest/}"
LIST="${LIST:-x}"                          # the list member's element name
XML_CT="${XML_CT:-text/xml}"               # the content type Coral's handler parses

OPEN="<$ROOT xmlns=\"$NS\">"
CLOSE="</$ROOT>"
GOOD="$OPEN<$LIST><member>a</member></$LIST>$CLOSE"

# probe <id> <description> <generated serde smithy-rs> <schema serde smithy-rs> <curl args...>
probe() {
  local id=$1 what=$2 legacy=$3 schema=$4
  shift 4
  echo "=================================================================="
  echo "$id  $what"
  echo "  legacy smithy-rs: $legacy"
  echo "  schema smithy-rs: $schema"
  echo "  request: curl $*"
  echo "  --- Coral response ---"
  curl -s -i --max-time 10 "$@" | sed 's/^/  /'
  echo
}

post() { # post <content-type or -> <body>
  local ct=$1 body=$2
  if [ "$ct" = "-" ]; then
    # `Header:` makes curl send no such header at all.
    printf '%s\n' -X POST "$BASE$OP_PATH" --data-binary "$body" -H "Content-Type:"
  elif [ -z "$ct" ]; then
    # `Header;` makes curl send the header with an empty value.
    printf '%s\n' -X POST "$BASE$OP_PATH" --data-binary "$body" -H "Content-Type;"
  else
    printf '%s\n' -X POST "$BASE$OP_PATH" --data-binary "$body" -H "Content-Type: $ct"
  fi
}

run() { # run <id> <what> <generated serde> <schema serde> <content-type> <body> [extra curl args...]
  local id=$1 what=$2 legacy=$3 schema=$4 ct=$5 body=$6
  shift 6
  local args=()
  while IFS= read -r line; do args+=("$line"); done < <(post "$ct" "$body")
  probe "$id" "$what" "$legacy" "$schema" "${args[@]}" "$@"
}

# ---------------------------------------------------------------------------------------------
# A. Ill-formed XML. The generated serde server recovers and runs the handler; schema serde answers 400.
#    For each: does Coral run the handler (and with which list), or reject, and with what status/body?

run A0 "baseline: well-formed list" \
  "200, list [a]" "200, list [a]" \
  "$XML_CT" "$GOOD"

run A1 "text after the root element" \
  "200, list [a]" "400" \
  "$XML_CT" "${GOOD}junk"

run A2 "second root element" \
  "200, first root read (list absent)" "400" \
  "$XML_CT" "<$ROOT xmlns=\"$NS\"/>$GOOD"

run A3 "stray close tag after the root" \
  "200, list [a]" "400" \
  "$XML_CT" "$GOOD$CLOSE"

run A4 "unclosed root (document ends early)" \
  "200, list [a]" "400" \
  "$XML_CT" "$OPEN<$LIST><member>a</member></$LIST>"

run A5 "unclosed child element" \
  "200, list [a]" "400" \
  "$XML_CT" "$OPEN<$LIST><member>a</$LIST>$CLOSE"

run A6 "mismatched end tag" \
  "200, list [a]" "400" \
  "$XML_CT" "$OPEN<$LIST><member>a</membre></$LIST>$CLOSE"

run A7 "document truncated in the middle of a tag" \
  "200, list [a]" "400" \
  "$XML_CT" "$OPEN<$LIST><member>a</mem"

run A8 "unescaped < in text" \
  "200, list [a]" "400" \
  "$XML_CT" "$OPEN<$LIST><member>a<b</member></$LIST>$CLOSE"

# Both smithy-rs servers already agree on the next three; Coral's answer tells us whether they match it.
run A9 "wrong root element name" \
  "400" "400" \
  "$XML_CT" "<Wrong xmlns=\"$NS\"><$LIST><member>a</member></$LIST></Wrong>"

run A10 "whitespace-only body" \
  "400" "400" \
  "$XML_CT" "   "

run A11 "JSON body sent as XML" \
  "400" "400" \
  "$XML_CT" '{"x":["a"]}'

# ---------------------------------------------------------------------------------------------
# B. Content-Type handling. Record status and whether the handler ran.

run B1 "empty body, XML content type" \
  "200, list absent" "200, list absent" \
  "$XML_CT" ""

run B2 "empty body, no Content-Type header" \
  "200, list absent" "200, list absent" \
  "-" ""

run B3 "empty body, Content-Type: text/plain" \
  "200, list absent (content type not checked without a body)" "404 <UnknownOperationException/> (no protocol claims it)" \
  "text/plain" ""

run B4 "empty body, empty Content-Type value" \
  "200, list absent" "404 <UnknownOperationException/>" \
  "" ""

run B5 "XML body, Content-Type: text/plain" \
  "415" "404 <UnknownOperationException/>" \
  "text/plain" "$GOOD"

run B6 "XML body, no Content-Type header" \
  "415" "415" \
  "-" "$GOOD"

run B7 "XML body, Content-Type: application/xml (the smithy-rs restXml type)" \
  "200, list [a]" "200, list [a]" \
  "application/xml" "$GOOD"

run B8 "XML body, Content-Type: text/xml (the type the Coral handler was tested with)" \
  "415" "404 <UnknownOperationException/>" \
  "text/xml" "$GOOD"

run B9 "XML body, content type with a charset parameter" \
  "200 (for application/xml; charset=utf-8)" "200 (for application/xml; charset=utf-8)" \
  "$XML_CT; charset=utf-8" "$GOOD"

# ---------------------------------------------------------------------------------------------
# C. Error responses and Accept. Record the status, the Content-Type header and the exact body.
#    Both smithy-rs servers answer a restXml framework error with the literal body `{}` under
#    `Content-Type: application/xml`, and answer an unacceptable Accept with 400 (not 406).

run C1 "Accept: application/json on an XML operation" \
  "400, body {}" "400, body {}" \
  "$XML_CT" "$GOOD" -H "Accept: application/json"

run C2 "Accept: */*" \
  "200" "200" \
  "$XML_CT" "$GOOD" -H "Accept: */*"

run C3 "Accept: text/xml" \
  "400, body {}" "400, body {}" \
  "$XML_CT" "$GOOD" -H "Accept: text/xml"

run C4 "Accept: application/xml" \
  "200" "200" \
  "$XML_CT" "$GOOD" -H "Accept: application/xml"

run C5 "the error body Coral sends for a malformed request (compare with A9)" \
  "400, Content-Type application/xml, body {}" "400, Content-Type application/xml, body {}" \
  "$XML_CT" "<Wrong/>"

# ---------------------------------------------------------------------------------------------
# D. Routing. Record status, headers and body.

probe D1 "unknown path, GET, no body" \
  "404, Content-Type application/xml, empty body" "404, no Content-Type, body <UnknownOperationException/>" \
  -X GET "$BASE/nope"

probe D2 "unknown path, POST with an XML body" \
  "404, Content-Type application/xml, empty body" "404, no Content-Type, body <UnknownOperationException/>" \
  -X POST "$BASE/nope" -H "Content-Type: $XML_CT" --data-binary "$GOOD"

probe D3 "known path, wrong method (PUT)" \
  "405, empty body" "404, body <UnknownOperationException/>" \
  -X PUT "$BASE$OP_PATH" -H "Content-Type: $XML_CT" --data-binary "$GOOD"

probe D4 "known path, wrong method (GET)" \
  "405, empty body" "404, body <UnknownOperationException/>" \
  -X GET "$BASE$OP_PATH"

# D5 matters only if the Coral service also has a JSON protocol handler in its chain: which protocol
# answers a request that carries neither a body nor a Content-Type? A schema server serving both
# restJson1 and restXml answers it as restJson1 (a JSON response).
probe D5 "known path, POST, no body, no Content-Type: which protocol frames the response?" \
  "restXml-only server: 200 with an XML body" "with restJson1 also served: 200 with a JSON body" \
  -X POST "$BASE$OP_PATH" -H "Content-Type:"
