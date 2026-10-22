$version: "2"
namespace compat.documentdepth
use aws.protocols#restJson1
use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1

@restJson1
service RestDepth { version: "1", operations: [Body, DocumentPayload] }
@awsJson1_0
service Json10Depth { version: "1", operations: [Body] }
@awsJson1_1
service Json11Depth { version: "1", operations: [Body] }

@http(method: "POST", uri: "/body")
operation Body { input := { document: Document }, output := {} }
@http(method: "POST", uri: "/document-payload")
operation DocumentPayload { input := { @httpPayload document: Document }, output := {} }
