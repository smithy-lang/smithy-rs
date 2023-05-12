$version: "1.0"
namespace crate

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests
use aws.protocols#awsJson1_1
use aws.api#service
use smithy.framework#ValidationException

/// Confounds model generation machinery by using operations named after every item in the Rust prelude
@awsJson1_1
@service(sdkId: "Config")
service Config {
    version: "2006-03-01",
    operations: [
       // Rust Prelude
       Copy,
       Send,
       Sized,
       Sync,
       Unpin,
       Drop,
       Fn,
       FnMut,
       FnOnce,
       Box,
       ToOwned,
       Clone,
       PartialEq,
       PartialOrd,
       Eq,
       Ord,
       AsRef,
       AsMut,
       Into,
       From,
       Default,
       Iterator,
       Extend,
       IntoIterator,
       DoubleEndedIterator,
       ExactSizeIterator,
       Option,
       Some,
       None,
       Result,
       Ok,
       Err,
       String,
       ToString,
       Vec,
    ]
}

structure Input {}
structure Output {}

operation Copy { input: Input, output: Output }
operation Send { input: Input, output: Output }
operation Sized { input: Input, output: Output }
operation Sync { input: Input, output: Output }
operation Unpin { input: Input, output: Output }
operation Drop { input: Input, output: Output }
operation Fn { input: Input, output: Output }
operation FnMut { input: Input, output: Output }
operation FnOnce { input: Input, output: Output }
operation Box { input: Input, output: Output }
operation ToOwned { input: Input, output: Output }
operation Clone { input: Input, output: Output }
operation PartialEq { input: Input, output: Output }
operation PartialOrd { input: Input, output: Output }
operation Eq { input: Input, output: Output }
operation Ord { input: Input, output: Output }
operation AsRef { input: Input, output: Output }
operation AsMut { input: Input, output: Output }
operation Into { input: Input, output: Output }
operation From { input: Input, output: Output }
operation Default { input: Input, output: Output }
operation Iterator { input: Input, output: Output }
operation Extend { input: Input, output: Output }
operation IntoIterator { input: Input, output: Output }
operation DoubleEndedIterator { input: Input, output: Output }
operation ExactSizeIterator { input: Input, output: Output }
operation Option { input: Input, output: Output }
operation Some { input: Input, output: Output }
operation None { input: Input, output: Output }
operation Result { input: Input, output: Output }
operation Ok { input: Input, output: Output }
operation Err { input: Input, output: Output }
operation String { input: Input, output: Output }
operation ToString { input: Input, output: Output }
operation Vec { input: Input, output: Output }
