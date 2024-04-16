$version: "2.0"

namespace aws.protocoltests.json10

use aws.api#service
use aws.protocols#awsJson1_0
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@service(sdkId: "JSON RPC 10")
@awsJson1_0
@title("Sample Json 1.0 Protocol Service")
service JsonRpc10Extras {
	version: "2024-04-15",
	operations: [
		ContentTypeParameters
	]
}

@httpRequestTests([
	{
		id: "AwsJson10ContentTypeParameters",
		documentation: "A server should ignore parameters added to the content type",
		protocol: awsJson1_0,
		method: "POST",
		headers: {
			"Content-Type": "application/x-amz-json-1.0; charset=utf-8",
			"X-Amz-Target": "JsonRpc10Extras.ContentTypeParameters",
		},
		uri: "/",
		body: "{\"value\":5}",
		bodyMediaType: "application/json",
		params: {
			value: 5,
		},
		appliesTo: "server"
	}
])
operation ContentTypeParameters {
	input: ContentTypeParametersInput,
	output: ContentTypeParametersOutput
}

@input
structure ContentTypeParametersInput {
	value: Integer,
}

@output
structure ContentTypeParametersOutput {}
