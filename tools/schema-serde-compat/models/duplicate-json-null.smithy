$version: "2"
namespace compat.duplicatenull
use aws.protocols#restJson1
use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1
@restJson1
service RestDuplicateNull { version: "1", operations: [Echo] }
@awsJson1_0
service Json10DuplicateNull { version: "1", operations: [Echo] }
@awsJson1_1
service Json11DuplicateNull { version: "1", operations: [Echo] }
@http(method: "POST", uri: "/echo")
operation Echo {
    input := { text: String, number: Integer }
    output := { @httpPayload value: String }
}
