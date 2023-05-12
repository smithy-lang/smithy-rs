$version: "1.0"
namespace crate

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests
use aws.protocols#awsJson1_1
use aws.api#service
use smithy.framework#ValidationException

/// Confounds model generation machinery by using structs named after every item in the Rust prelude
@awsJson1_1
@service(sdkId: "Config")
service Config {
    version: "2006-03-01",
    operations: [
       UseCopy,
       UseSend,
       UseSized,
       UseSync,
       UseUnpin,
       UseDrop,
       UseFn,
       UseFnMut,
       UseFnOnce,
       UseBox,
       UseToOwned,
       UseClone,
       UsePartialEq,
       UsePartialOrd,
       UseEq,
       UseOrd,
       UseAsRef,
       UseAsMut,
       UseInto,
       UseFrom,
       UseDefault,
       UseIterator,
       UseExtend,
       UseIntoIterator,
       UseDoubleEndedIterator,
       UseExactSizeIterator,
       UseOption,
       UseSome,
       UseNone,
       UseResult,
       UseOk,
       UseErr,
       UseString,
       UseToString,
       UseVec,
    ]
}

// Rust Prelude
structure Copy {}
structure Send {}
structure Sized {}
structure Sync {}
structure Unpin {}
structure Drop {}
structure Fn {}
structure FnMut {}
structure FnOnce {}
structure Box {}
structure ToOwned {}
structure Clone {}
structure PartialEq {}
structure PartialOrd {}
structure Eq {}
structure Ord {}
structure AsRef {}
structure AsMut {}
structure Into {}
structure From {}
structure Default {}
structure Iterator {}
structure Extend {}
structure IntoIterator {}
structure DoubleEndedIterator {}
structure ExactSizeIterator {}
structure Option {}
structure Some {}
structure None {}
structure Result {}
structure Ok {}
structure Err {}
structure String {}
structure ToString {}
structure Vec {}

operation UseCopy { input: Copy, output: Copy }
operation UseSend { input: Send, output: Send }
operation UseSized { input: Sized, output: Sized }
operation UseSync { input: Sync, output: Sync }
operation UseUnpin { input: Unpin, output: Unpin }
operation UseDrop { input: Drop, output: Drop }
operation UseFn { input: Fn, output: Fn }
operation UseFnMut { input: FnMut, output: FnMut }
operation UseFnOnce { input: FnOnce, output: FnOnce }
operation UseBox { input: Box, output: Box }
operation UseToOwned { input: ToOwned, output: ToOwned }
operation UseClone { input: Clone, output: Clone }
operation UsePartialEq { input: PartialEq, output: PartialEq }
operation UsePartialOrd { input: PartialOrd, output: PartialOrd }
operation UseEq { input: Eq, output: Eq }
operation UseOrd { input: Ord, output: Ord }
operation UseAsRef { input: AsRef, output: AsRef }
operation UseAsMut { input: AsMut, output: AsMut }
operation UseInto { input: Into, output: Into }
operation UseFrom { input: From, output: From }
operation UseDefault { input: Default, output: Default }
operation UseIterator { input: Iterator, output: Iterator }
operation UseExtend { input: Extend, output: Extend }
operation UseIntoIterator { input: IntoIterator, output: IntoIterator }
operation UseDoubleEndedIterator { input: DoubleEndedIterator, output: DoubleEndedIterator }
operation UseExactSizeIterator { input: ExactSizeIterator, output: ExactSizeIterator }
operation UseOption { input: Option, output: Option }
operation UseSome { input: Some, output: Some }
operation UseNone { input: None, output: None }
operation UseResult { input: Result, output: Result }
operation UseOk { input: Ok, output: Ok }
operation UseErr { input: Err, output: Err }
operation UseString { input: String, output: String }
operation UseToString { input: ToString, output: ToString }
operation UseVec { input: Vec, output: Vec }
