#curl -vv --http2-prior-knowledge localhost:8000/sample -X POST -H 'content-type: application/json' -d '{"inputValue": "some value"}'
curl -vv --http2-prior-knowledge \
  -X POST \
  -H "Smithy-Protocol: rpc-v2-cbor" \
  -H "Content-Type: application/cbor" \
  -H "Accept: application/cbor" \
  --data-binary @input.cbor \
  localhost:8000/service/SampleService/operation/SampleOperation

