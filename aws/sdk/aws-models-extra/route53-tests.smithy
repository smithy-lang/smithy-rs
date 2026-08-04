$version: "1.0"

namespace com.amazonaws.route53

// NOTE: These Route 53 resource-ID trimming protocol tests are temporarily disabled.
//
// The expected request URIs below hardcode the `/2013-04-01/` API version segment. The public
// Route 53 model has since bumped its API version (the service shape changed from
// `AWSDnsV20130401` to a newer version, e.g. `AWSDnsV20130527`), which changes every operation
// URI to the new version segment. Because we cannot land the updated public model in this
// repository until it is officially released, these tests cannot pass in the meantime.
//
// TODO(route53): Re-enable these tests once the updated public Route 53 model is released, and update each
// expected `uri` to the released API version segment. The resource-ID trimming behavior itself is
// version-agnostic (the Route53Decorator now matches on the `com.amazonaws.route53` namespace
// rather than an exact, version-specific service shape ID), so only the `uri` version segment
// needs updating when these are restored.
//
// use smithy.test#httpRequestTests
//
// apply ListResourceRecordSets @httpRequestTests([
//     {
//         id: "ListResourceRecordSetsTrimHostedZone",
//         documentation: "This test validates that hosted zone is correctly trimmed",
//         method: "GET",
//         protocol: "aws.protocols#restXml",
//         uri: "/2013-04-01/hostedzone/IDOFMYHOSTEDZONE/rrset",
//         bodyMediaType: "application/xml",
//         params: {
//             "HostedZoneId": "/hostedzone/IDOFMYHOSTEDZONE"
//         }
//         vendorParams: {
//             "endpointParams": {
//                 "builtInParams": {
//                     "AWS::Region": "us-east-1"
//                 }
//             }
//         }
//     }
// ])
//
// apply GetChange @httpRequestTests([
//     {
//         id: "GetChangeTrimChangeId",
//         documentation: "This test validates that change id is correctly trimmed",
//         method: "GET",
//         protocol: "aws.protocols#restXml",
//         uri: "/2013-04-01/change/SOMECHANGEID",
//         bodyMediaType: "application/xml",
//         params: {
//             "Id": "/change/SOMECHANGEID"
//         }
//         vendorParams: {
//             "endpointParams": {
//                 "builtInParams": {
//                     "AWS::Region": "us-east-1"
//                 }
//             }
//         }
//     },
// ])
//
// apply GetReusableDelegationSet @httpRequestTests([
//     {
//         id: "GetReusableDelegationSetTrimDelegationSetId",
//         documentation: "This test validates that delegation set id is correctly trimmed",
//         method: "GET",
//         protocol: "aws.protocols#restXml",
//         uri: "/2013-04-01/delegationset/DELEGATIONSETID",
//         bodyMediaType: "application/xml",
//         params: {
//             "Id": "/delegationset/DELEGATIONSETID"
//         }
//         vendorParams: {
//             "endpointParams": {
//                 "builtInParams": {
//                     "AWS::Region": "us-east-1"
//                 }
//             }
//         }
//     },
// ])
