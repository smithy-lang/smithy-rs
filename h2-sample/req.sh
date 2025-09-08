#!/bin/bash

# Default values
HOST="localhost"
PORT="8000"
SERVICE="SampleService"
OPERATION="SampleOperation"
JSON_INPUT='{"inputValue": "some value"}'
VERBOSE=""
KEEP_FILES=false

# Parse named arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --host)
      HOST="$2"
      shift 2
      ;;
    --port)
      PORT="$2"
      shift 2
      ;;
    --service)
      SERVICE="$2"
      shift 2
      ;;
    --operation)
      OPERATION="$2"
      shift 2
      ;;
    --json)
      JSON_INPUT="$2"
      shift 2
      ;;
    --verbose)
      VERBOSE="-vv"
      shift
      ;;
    --keep)
      KEEP_FILES=true
      shift
      ;;
    *)
      echo "Unknown option $1"
      exit 1
      ;;
  esac
done

# Convert JSON to CBOR using Python
python3 -c "
import json
import sys
try:
    import cbor2
except ImportError:
    print('Error: cbor2 package not installed. Install with: pip3 install cbor2')
    sys.exit(1)

json_data = json.loads('$JSON_INPUT')
cbor_data = cbor2.dumps(json_data)
with open('input.cbor', 'wb') as f:
    f.write(cbor_data)
"

if [ $? -ne 0 ]; then
    echo "Failed to convert JSON to CBOR"
    exit 1
fi

# Show the exact curl command
echo "=== Curl Command ==="
echo "curl $VERBOSE --http2-prior-knowledge \\"
echo "  -X POST \\"
echo "  -H \"Smithy-Protocol: rpc-v2-cbor\" \\"
echo "  -H \"Content-Type: application/cbor\" \\"
echo "  -H \"Accept: application/cbor\" \\"
echo "  --data-binary @input.cbor \\"
echo "  -D response_headers.txt \\"
echo "  -o response_body.cbor \\"
echo "  ${HOST}:${PORT}/service/${SERVICE}/operation/${OPERATION}"
echo

# Make request and save response with headers
curl $VERBOSE --http2-prior-knowledge \
  -X POST \
  -H "Smithy-Protocol: rpc-v2-cbor" \
  -H "Content-Type: application/cbor" \
  -H "Accept: application/cbor" \
  --data-binary @input.cbor \
  -D response_headers.txt \
  -o response_body.cbor \
  ${HOST}:${PORT}/service/${SERVICE}/operation/${OPERATION}

# Check if request was successful
if [ $? -ne 0 ]; then
    echo "Request failed"
    exit 1
fi

# Check content-type header
CONTENT_TYPE=$(grep -i "content-type:" response_headers.txt | cut -d' ' -f2- | tr -d '\r\n')
if [[ "$CONTENT_TYPE" != *"application/cbor"* ]]; then
    echo "Error: Expected application/cbor response, got: $CONTENT_TYPE"
    cat response_body.cbor
    exit 1
fi

# Print hex dump of CBOR response
echo "=== CBOR Response (hex) ==="
hexdump -C response_body.cbor

# Generate CBOR analyzer link
HEX_VALUE=$(hexdump -ve '1/1 "%.2x"' response_body.cbor)
echo "=== CBOR Analyzer Link ==="
echo "https://cbor.nemo157.com/#type=hex&value=$HEX_VALUE"

# Convert CBOR response back to JSON
echo "=== JSON Response ==="
python3 -c "
import json
import sys
try:
    import cbor2
except ImportError:
    print('Error: cbor2 package not installed')
    sys.exit(1)

try:
    with open('response_body.cbor', 'rb') as f:
        cbor_data = f.read()
    json_data = cbor2.loads(cbor_data)
    print(json.dumps(json_data, indent=2))
except Exception as e:
    print(f'Error converting CBOR to JSON: {e}')
    sys.exit(1)
"

# Clean up temporary files unless --keep is specified
if [ "$KEEP_FILES" = false ]; then
    rm -f input.cbor response_headers.txt response_body.cbor
else
    echo "=== Files Kept ==="
    echo "Input: input.cbor"
    echo "Response headers: response_headers.txt"
    echo "Response body: response_body.cbor"
fi

