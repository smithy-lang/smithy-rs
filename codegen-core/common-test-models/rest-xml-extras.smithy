$version: "2.0"

namespace aws.protocoltests.restxml

use aws.api#service
use aws.protocols#restXml
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

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

@httpRequestTests([
	{
		id: "RestXmlContentTypeParameters",
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
@http(uri: "/ContentTypeParameters", method: "PUT")
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
