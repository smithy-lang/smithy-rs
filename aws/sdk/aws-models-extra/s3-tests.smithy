$version: "1.0"

namespace com.amazonaws.s3

use smithy.test#httpResponseTests
use smithy.test#httpRequestTests

apply NotFound @httpResponseTests([
    {
        id: "HeadObjectEmptyBody",
        documentation: "This test case validates https://github.com/smithy-lang/smithy-rs/issues/456",
        params: {
        },
        bodyMediaType: "application/xml",
        body: "",
        protocol: "aws.protocols#restXml",
        code: 404,
        headers: {
            "x-amz-request-id": "GRZ6BZ468DF52F2E",
            "x-amz-id-2": "UTniwu6QmCIjVeuK2ZfeWBOnu7SqMQOS3Vac6B/K4H2ZCawYUl+nDbhGTImuyhZ5DFiojR3Kcz4=",
            "content-type": "application/xml",
            "date": "Thu, 03 Jun 2021 04:05:52 GMT",
            "server": "AmazonS3"
        }
    }
])


apply GetBucketLocation @httpResponseTests([
    {
        id: "GetBucketLocation",
        documentation: "This test case validates https://github.com/awslabs/aws-sdk-rust/issues/116",
        code: 200,
        body: "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<LocationConstraint xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">us-west-2</LocationConstraint>",
        params: {
            "LocationConstraint": "us-west-2"
        },
        bodyMediaType: "application/xml",
        protocol: "aws.protocols#restXml"
    }
])

apply ListObjects @httpResponseTests([
    {
        id: "KeysWithWhitespace",
        documentation: "This test validates that parsing respects whitespace",
        code: 200,
        bodyMediaType: "application/xml",
        protocol: "aws.protocols#restXml",
        body: """
        <?xml version=\"1.0\" encoding=\"UTF-8\"?>\n
        <ListBucketResult
        	xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">
        	<Name>bucketname</Name>
        	<Prefix></Prefix>
        	<Marker></Marker>
        	<MaxKeys>1000</MaxKeys>
        	<IsTruncated>false</IsTruncated>
        	<Contents>
        		<Key>    </Key>
        		<LastModified>2021-07-16T16:20:53.000Z</LastModified>
        		<ETag>&quot;etag123&quot;</ETag>
        		<Size>0</Size>
        		<Owner>
        			<ID>owner</ID>
        		</Owner>
        		<StorageClass>STANDARD</StorageClass>
        	</Contents>
        	<Contents>
        		<Key> a </Key>
        		<LastModified>2021-07-16T16:02:10.000Z</LastModified>
        		<ETag>&quot;etag123&quot;</ETag>
        		<Size>0</Size>
        		<Owner>
        			<ID>owner</ID>
        		</Owner>
        		<StorageClass>STANDARD</StorageClass>
        	</Contents>
        </ListBucketResult>
        """,
        params: {
            MaxKeys: 1000,
            IsTruncated: false,
            Marker: "",
            Name: "bucketname",
            Prefix: "",
            Contents: [{
                           Key: "    ",
                           LastModified: 1626452453,
                           ETag: "\"etag123\"",
                           Size: 0,
                           Owner: { ID: "owner" },
                           StorageClass: "STANDARD"
                       }, {
                           Key: " a ",
                           LastModified: 1626451330,
                           ETag: "\"etag123\"",
                           Size: 0,
                           Owner: { ID: "owner" },
                           StorageClass: "STANDARD"
                       }]
        }
    }
])

apply PutBucketLifecycleConfiguration @httpRequestTests([
    {
        id: "PutBucketLifecycleConfiguration",
        documentation: "This test validates that the content md5 header is set correctly",
        method: "PUT",
        protocol: "aws.protocols#restXml",
        uri: "/",
        headers: {
            "x-amz-checksum-crc32": "11+f3g==",
        },
        bodyMediaType: "application/xml",
        body: """
        <LifecycleConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">
            <Rule>
                <Expiration>
                    <Days>1</Days>
                </Expiration>
                <ID>Expire</ID>
                <Status>Enabled</Status>
            </Rule>
        </LifecycleConfiguration>
        """,
        params: {
            "Bucket": "test-bucket",
            "LifecycleConfiguration": {
                "Rules": [
                    {"Expiration": { "Days": 1 }, "Status": "Enabled", "ID": "Expire" },
                ]
            }
        },
        vendorParams: {
            "endpointParams": {
                "builtInParams": {
                    "AWS::Region": "us-east-1"
                }
            }
        }
    }
])

apply CreateMultipartUpload @httpRequestTests([
    {
        id: "CreateMultipartUploadUriConstruction",
        documentation: "This test validates that the URI for CreateMultipartUpload is created correctly",
        method: "POST",
        protocol: "aws.protocols#restXml",
        uri: "/object.txt",
        queryParams: [
            "uploads"
        ],
        params: {
            "Bucket": "test-bucket",
            "Key": "object.txt"
        },
        vendorParams: {
            "endpointParams": {
                "builtInParams": {
                    "AWS::Region": "us-east-1"
                }
            }
        }
    }
])

apply PutObject @httpRequestTests([
    {
        id: "DontSendDuplicateContentType",
        documentation: "This test validates that if a content-type is specified, that only one content-type header is sent",
        method: "PUT",
        protocol: "aws.protocols#restXml",
        uri: "/test-key",
        headers: { "content-type": "text/html" },
        params: {
            Bucket: "test-bucket",
            Key: "test-key",
            ContentType: "text/html"
        },
        vendorParams: {
            "endpointParams": {
                "builtInParams": {
                    "AWS::Region": "us-east-1"
                }
            }
        }
    },
    {
        id: "DontSendDuplicateContentLength",
        documentation: "This test validates that if a content-length is specified, that only one content-length header is sent",
        method: "PUT",
        protocol: "aws.protocols#restXml",
        uri: "/test-key",
        headers: { "content-length": "2" },
        params: {
            Bucket: "test-bucket",
            Key: "test-key",
            ContentLength: 2,
            Body: "ab"
        },
        vendorParams: {
            "endpointParams": {
                "builtInParams": {
                    "AWS::Region": "us-east-1"
                }
            }
        }
    }
])

apply HeadObject @httpRequestTests([
    {
        id: "HeadObjectUriEncoding",
        documentation: "https://github.com/awslabs/aws-sdk-rust/issues/331",

        method: "HEAD",
        protocol: "aws.protocols#restXml",
        uri: "/%3C%3E%20%60%3F%F0%9F%90%B1",
        params: {
            Bucket: "test-bucket",
            Key: "<> `?🐱",
        },
        vendorParams: {
            "endpointParams": {
                "builtInParams": {
                    "AWS::Region": "us-east-1"
                }
            }
        }
    }
])

apply GetObject @httpRequestTests([
    {
        id: "GetObjectIfModifiedSince",
        documentation: "https://github.com/awslabs/aws-sdk-rust/issues/818",

        method: "GET",
        protocol: "aws.protocols#restXml",
        uri: "/object.txt",
        headers: { "if-modified-since": "Fri, 16 Jul 2021 16:20:53 GMT" }
        params: {
            Bucket: "test-bucket",
            Key: "object.txt"
            IfModifiedSince: 1626452453.123,
        },
        vendorParams: {
            "endpointParams": {
                "builtInParams": {
                    "AWS::Region": "us-east-1"
                }
            }
        }
    }
])

apply ListObjectVersions @httpResponseTests([
    {
        id: "OutOfOrderVersions",
        documentation: "Verify that interleaving list elements (DeleteMarker and Version) from different lists works",
        code: 200,
        bodyMediaType: "application/xml",
        protocol: "aws.protocols#restXml",
        body: """
        <?xml version="1.0"?>
        <ListVersionsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
          <Name>sdk-obj-versions-test</Name>
          <Prefix/>
          <KeyMarker/>
          <VersionIdMarker/>
          <MaxKeys>1000</MaxKeys>
          <IsTruncated>false</IsTruncated>
          <DeleteMarker>
            <Key>build.gradle.kts</Key>
            <VersionId>null</VersionId>
            <IsLatest>true</IsLatest>
            <LastModified>2009-02-13T23:31:30Z</LastModified>
            <Owner>
              <ID>c1665459250c459f1849ddce9b291fc3a72bcf5220dc8f6391a0a1045c683b34</ID>
              <DisplayName>test-name</DisplayName>
            </Owner>
          </DeleteMarker>
          <Version>
            <Key>build.gradle.kts</Key>
            <VersionId>IfK9Z4.H5TLAtMxFrxN_C7rFEZbufF3V</VersionId>
            <IsLatest>false</IsLatest>
            <LastModified>2009-02-13T23:31:30Z</LastModified>
            <ETag>"99613b85e3f38b222c4ee548cde1e59d"</ETag>
            <Size>6903</Size>
            <Owner>
              <ID>c1665459250c459f1849ddce9b291fc3a72bcf5220dc8f6391a0a1045c683b34</ID>
              <DisplayName>test-name</DisplayName>
            </Owner>
            <StorageClass>STANDARD</StorageClass>
          </Version>
          <DeleteMarker>
            <Key>file-2</Key>
            <VersionId>o98RL6vmlOYiymftbX7wgy_4XWQG4AmY</VersionId>
            <IsLatest>true</IsLatest>
            <LastModified>2009-02-13T23:31:30Z</LastModified>
            <Owner>
              <ID>c1665459250c459f1849ddce9b291fc3a72bcf5220dc8f6391a0a1045c683b34</ID>
              <DisplayName>test-name</DisplayName>
            </Owner>
          </DeleteMarker>
          <Version>
            <Key>file-2</Key>
            <VersionId>PSVAbvQihRdsNiktGothjGng7q.5ou9Q</VersionId>
            <IsLatest>false</IsLatest>
            <LastModified>2009-02-13T23:31:30Z</LastModified>
            <ETag>"1727d9cb38dd325d9c12c973ef3675fc"</ETag>
            <Size>14</Size>
            <Owner>
              <ID>c1665459250c459f1849ddce9b291fc3a72bcf5220dc8f6391a0a1045c683b34</ID>
              <DisplayName>test-name</DisplayName>
            </Owner>
            <StorageClass>STANDARD</StorageClass>
          </Version>
        </ListVersionsResult>
        """,
            "params": {
                "Name": "sdk-obj-versions-test",
                "Prefix": "",
                "KeyMarker": "",
                "VersionIdMarker": "",
                "MaxKeys": 1000,
                "IsTruncated": false,
                "DeleteMarkers": [
                    {
                        "Key": "build.gradle.kts",
                        "VersionId": "null",
                        "IsLatest": true,
                        "LastModified": 1234567890,
                        "Owner": {
                            "ID": "c1665459250c459f1849ddce9b291fc3a72bcf5220dc8f6391a0a1045c683b34",
                            "DisplayName": "test-name"
                        }
                    },
                    {
                        "Key": "file-2",
                        "VersionId": "o98RL6vmlOYiymftbX7wgy_4XWQG4AmY",
                        "IsLatest": true,
                        "LastModified": 1234567890,
                        "Owner": {
                            "ID": "c1665459250c459f1849ddce9b291fc3a72bcf5220dc8f6391a0a1045c683b34",
                            "DisplayName": "test-name"
                        }
                    }
                ],
                "Versions": [
                    {
                        "Key": "build.gradle.kts",
                        "VersionId": "IfK9Z4.H5TLAtMxFrxN_C7rFEZbufF3V",
                        "IsLatest": false,
                        "LastModified": 1234567890,
                        "ETag": "\"99613b85e3f38b222c4ee548cde1e59d\"",
                        "Size": 6903,
                        "Owner": {
                            "ID": "c1665459250c459f1849ddce9b291fc3a72bcf5220dc8f6391a0a1045c683b34",
                            "DisplayName": "test-name"
                        },
                        "StorageClass": "STANDARD"
                    },
                    {
                        "Key": "file-2",
                        "VersionId": "PSVAbvQihRdsNiktGothjGng7q.5ou9Q",
                        "IsLatest": false,
                        "LastModified": 1234567890,
                        "ETag": "\"1727d9cb38dd325d9c12c973ef3675fc\"",
                        "Size": 14,
                        "Owner": {
                            "ID": "c1665459250c459f1849ddce9b291fc3a72bcf5220dc8f6391a0a1045c683b34",
                            "DisplayName": "test-name"
                        },
                        "StorageClass": "STANDARD"
                    }
                ]
            }
}]
)


// TODO(https://github.com/smithy-lang/smithy-rs/issues/157) - Remove duplicated tests if these make it into the actual model or otherwise become easier
// to integrate.
// Protocol tests below are duplicated from
// https://github.com/smithy-lang/smithy/blob/main/smithy-aws-protocol-tests/model/restXml/services/s3.smithy
// NOTE: These are duplicated because of currently difficult to replicate structural differences in the build.
// S3 pulls in `aws-config` which requires all runtime crates to point to `build` dir. This makes adding the protocol tests
// to `sdk-adhoc-test` difficult as it does not replicate relocating runtimes and re-processing Cargo.toml files.

apply DeleteObjectTagging @httpRequestTests([
    {
        id: "S3EscapeObjectKeyInUriLabel",
        documentation: """
            S3 clients should escape special characters in Object Keys
            when the Object Key is used as a URI label binding.
        """,
        protocol: "aws.protocols#restXml",
        method: "DELETE",
        uri: "/my%20key.txt",
        host: "s3.us-west-2.amazonaws.com",
        resolvedHost: "mybucket.s3.us-west-2.amazonaws.com",
        body: "",
        queryParams: [
            "tagging"
        ],
        params: {
            Bucket: "mybucket",
            Key: "my key.txt"
        },
        vendorParams: {
            scopedConfig: {
                client: {
                    region: "us-west-2",
                },
            },
        },
    },
    {
        id: "S3EscapePathObjectKeyInUriLabel",
        documentation: """
            S3 clients should preserve an Object Key representing a path
            when the Object Key is used as a URI label binding, but still
            escape special characters.
        """,
        protocol: "aws.protocols#restXml",
        method: "DELETE",
        uri: "/foo/bar/my%20key.txt",
        host: "s3.us-west-2.amazonaws.com",
        resolvedHost: "mybucket.s3.us-west-2.amazonaws.com",
        body: "",
        queryParams: [
            "tagging"
        ],
        params: {
            Bucket: "mybucket",
            Key: "foo/bar/my key.txt"
        },
        vendorParams: {
            scopedConfig: {
                client: {
                    region: "us-west-2",
                },
            },
        },
    }
])

apply GetObject @httpRequestTests([
    {
        id: "S3PreservesLeadingDotSegmentInUriLabel",
        documentation: """
            S3 clients should not remove dot segments from request paths.
        """,
        protocol: "aws.protocols#restXml",
        method: "GET",
        uri: "/../key.txt",
        host: "s3.us-west-2.amazonaws.com",
        resolvedHost: "mybucket.s3.us-west-2.amazonaws.com",
        body: "",
        params: {
            Bucket: "mybucket",
            Key: "../key.txt"
        },
        vendorParams: {
            scopedConfig: {
                client: {
                    region: "us-west-2",
                    s3: {
                        addressing_style: "virtual",
                    },
                },
            },
        },
    },
    {
        id: "S3PreservesEmbeddedDotSegmentInUriLabel",
        documentation: """
            S3 clients should not remove dot segments from request paths.
        """,
        protocol: "aws.protocols#restXml",
        method: "GET",
        uri: "/foo/../key.txt",
        host: "s3.us-west-2.amazonaws.com",
        resolvedHost: "mybucket.s3.us-west-2.amazonaws.com",
        body: "",
        params: {
            Bucket: "mybucket",
            Key: "foo/../key.txt"
        },
        vendorParams: {
            scopedConfig: {
                client: {
                    region: "us-west-2",
                    s3: {
                        addressing_style: "virtual",
                    },
                },
            },
        },
    }
])
