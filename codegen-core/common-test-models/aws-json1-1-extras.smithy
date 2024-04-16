$version: "2.0"

namespace aws.protocoltests.json

use aws.api#service
use aws.protocols#awsJson1_1
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@service(
	sdkId: "Json Protocol",
	arnNamespace: "jsonprotocol",
	cloudFormationName: "JsonProtocol",
	cloudTrailEventSource: "jsonprotocol.amazonaws.com",
)
@awsJson1_1
@title("Sample Json 1.1 Protocol Service")
service JsonProtocolExtras {
	version: "2024-04-15",
	operations: [
		ContentTypeParameters
	]
}

@httpRequestTests([
	{
		id: "AwsJson11ContentTypeParameters",
		documentation: "A server should ignore parameters added to the content type",
		protocol: awsJson1_1,
		method: "POST",
		headers: {
			"Content-Type": "application/x-amz-json-1.1; charset=utf-8",
			"X-Amz-Target": "JsonProtocolExtras.ContentTypeParameters",
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
