$version: "2"

namespace com.example.review

use aws.protocols#awsJson1_1
use aws.protocols#restXml

// Single-protocol fixtures for comparing custom extractor rejection responses against legacy.
@awsJson1_1
service ReviewAwsJson11 {
    version: "2024-01-01"
    operations: [GetWidget, DownloadBlob]
}

@restXml
service ReviewXml {
    version: "2024-01-01"
    operations: [GetWidget, DownloadBlob]
}
