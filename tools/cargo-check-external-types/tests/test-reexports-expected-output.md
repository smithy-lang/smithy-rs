error: Unapproved external type `external_lib::AssociatedGenericTrait` referenced in public API
 --> test-reexports-crate/src/lib.rs:6:1
  |
6 | pub use external_lib::AssociatedGenericTrait;
  | ^-------------------------------------------^
  |
  = in re-export named `test_reexports_crate::AssociatedGenericTrait`

error: Unapproved external type `external_lib::ReprCType` referenced in public API
 --> test-reexports-crate/src/lib.rs:7:1
  |
7 | pub use external_lib::ReprCType;
  | ^------------------------------^
  |
  = in re-export named `test_reexports_crate::ReprCType`

error: Unapproved external type `external_lib::SimpleTrait` referenced in public API
 --> test-reexports-crate/src/lib.rs:8:1
  |
8 | pub use external_lib::SimpleTrait;
  | ^--------------------------------^
  |
  = in re-export named `test_reexports_crate::SimpleTrait`

error: Unapproved external type `external_lib::SimpleGenericTrait` referenced in public API
  --> test-reexports-crate/src/lib.rs:11:5
   |
11 |     pub use external_lib::SimpleGenericTrait;
   |     ^---------------------------------------^
   |
   = in re-export named `test_reexports_crate::Something::SimpleGenericTrait`

error: Unapproved external type `external_lib::SimpleNewType` referenced in public API
  --> test-reexports-crate/src/lib.rs:12:5
   |
12 |     pub use external_lib::SimpleNewType;
   |     ^----------------------------------^
   |
   = in re-export named `test_reexports_crate::Something::SimpleNewType`

error: Unapproved external type `external_lib::SomeOtherStruct` referenced in public API
  --> test-reexports-crate/src/lib.rs:15:1
   |
15 | pub use external_lib::SomeOtherStruct;
   | ^------------------------------------^
   |
   = in re-export named `test_reexports_crate::SomeOtherStruct`

error: Unapproved external type `external_lib::SomeStruct` referenced in public API
  --> test-reexports-crate/src/lib.rs:16:1
   |
16 | pub use external_lib::SomeStruct;
   | ^-------------------------------^
   |
   = in re-export named `test_reexports_crate::SomeStruct`

7 errors emitted
