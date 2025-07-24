$version: "2"

namespace example.cacheable

use smithy.rust#cacheable

/// A service that uses the CBOR protocol and has cacheable members
@protocols([{name: "smithy.rpcv2.cbor"}])
service CacheableService {
    version: "2023-01-01",
    operations: [GetUserData]
}

/// Get user data operation
operation GetUserData {
    input: GetUserDataInput,
    output: GetUserDataOutput
}

/// Input for GetUserData operation
structure GetUserDataInput {
    /// User ID to retrieve data for
    userId: String
}

/// Output for GetUserData operation
structure GetUserDataOutput {
    /// User data that can be cached
    @cacheable
    userData: UserData,

    /// Request ID for tracing
    requestId: String
}

/// User data structure
structure UserData {
    /// User name
    name: String,

    /// User email
    email: String,

    /// User preferences that can be cached
    @cacheable
    preferences: UserPreferences
}

/// User preferences structure
structure UserPreferences {
    /// Theme preference
    theme: String,

    /// Language preference
    language: String,

    /// Notification settings
    notifications: Boolean
}

/// A list of users that can be cached
structure UserList {
    /// List of users
    @cacheable
    users: Users
}

/// List of user data
list Users {
    member: UserData
}
