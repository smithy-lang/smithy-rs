$version: "2.0"

namespace aws.protocoltests.restxml

use aws.api#service
use aws.protocols#restXml
use smithy.test#httpRequestTests

/// A REST XML service that sends XML requests and responses.
@service(sdkId: "Rest Xml Protocol")
@restXml
@title("Sample Rest Xml Protocol Service")
service RestXmlExtras {
	version: "2024-04-15",
	operations: [
		ContentTypeParameters
	]
}

/// The example tests how servers must support requests
/// containing a `Content-Type` header with parameters.
@http(uri: "/ContentTypeParameters", method: "PUT")
operation ContentTypeParameters {
	input: ContentTypeParametersInput,
	output: ContentTypeParametersOutput
}

apply ContentTypeParameters @httpRequestTests([
	{
		id: "RestXmlMustSupportParametersInContentType",
		documentation: "A server should ignore parameters added to the content type",
		protocol: restXml,
		method: "PUT",
		headers: {
			"Content-Type": "application/xml; charset=utf-8"
		},
		uri: "/ContentTypeParameters",
		body: "<ContentTypeParametersInput><value>5</value></ContentTypeParametersInput>",
		bodyMediaType: "application/xml",
		params: {
			value: 5,
		},
		appliesTo: "server"
	}
])

@input
structure ContentTypeParametersInput {
	value: Integer,
}

@output
structure ContentTypeParametersOutput {}
