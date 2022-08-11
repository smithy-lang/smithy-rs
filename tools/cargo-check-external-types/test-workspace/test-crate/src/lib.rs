/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![feature(generic_associated_types)]
#![allow(dead_code)]

//! This crate is used to test the cargo-check-external-types by exercising the all possible
//! exposure of external types in a public API.

use external_lib::{
    AssociatedGenericTrait,
    SimpleNewType,
    SimpleTrait,
    SomeOtherStruct,
    SomeStruct,
    // Remove this comment if more lines are needed for imports in the future to preserve line numbers
    // Remove this comment if more lines are needed for imports in the future to preserve line numbers
    // Remove this comment if more lines are needed for imports in the future to preserve line numbers
    // Remove this comment if more lines are needed for imports in the future to preserve line numbers
    // Remove this comment if more lines are needed for imports in the future to preserve line numbers
    // Remove this comment if more lines are needed for imports in the future to preserve line numbers
    // Remove this comment if more lines are needed for imports in the future to preserve line numbers
    // Remove this comment if more lines are needed for imports in the future to preserve line numbers
    // Remove this comment if more lines are needed for imports in the future to preserve line numbers
    // Remove this comment if more lines are needed for imports in the future to preserve line numbers
};

pub struct LocalStruct;

pub fn fn_with_local_struct(_local: LocalStruct) -> LocalStruct {
    unimplemented!()
}

fn not_pub_external_in_fn_input(_one: &SomeStruct, _two: impl SimpleTrait) {}

pub fn external_in_fn_input(_one: &SomeStruct, _two: impl SimpleTrait) {}

fn not_pub_external_in_fn_output() -> SomeStruct {
    unimplemented!()
}
pub fn external_in_fn_output() -> SomeStruct {
    unimplemented!()
}

pub fn external_opaque_type_in_output() -> impl SimpleTrait {
    unimplemented!()
}

fn not_pub_external_in_fn_output_generic() -> Option<SomeStruct> {
    unimplemented!()
}
pub fn external_in_fn_output_generic() -> Option<SomeStruct> {
    unimplemented!()
}

// Try to trick cargo-check-external-types here by putting something in a private module and re-exporting it
mod private_module {
    use external_lib::SomeStruct;

    pub fn something(_one: &SomeStruct) {}
}
pub use private_module::something;

pub struct StructWithExternalFields {
    pub field: SomeStruct,
    pub optional_field: Option<SomeStruct>,
}

impl StructWithExternalFields {
    pub fn new(_field: impl Into<SomeStruct>, _optional_field: Option<SomeOtherStruct>) -> Self {
        unimplemented!()
    }
}

pub trait TraitReferencingExternals {
    fn something(&self, a: SomeStruct) -> LocalStruct;
    fn optional_something(&self, a: Option<SomeStruct>) -> LocalStruct;
    fn otherthing(&self) -> SomeStruct;
    fn optional_otherthing(&self) -> Option<SomeStruct>;
}

pub enum EnumWithExternals<T = SomeStruct> {
    NormalTuple(LocalStruct),
    NormalStruct {
        v: LocalStruct,
    },
    TupleEnum(SomeStruct, Box<dyn SimpleTrait>),
    StructEnum {
        some_struct: SomeStruct,
        simple_trait: Box<dyn SimpleTrait>,
    },
    GenericTupleEnum(T),
    GenericStructEnum {
        t: T,
    },
}

impl<T> EnumWithExternals<T> {
    pub fn thing(_t: LocalStruct) -> Self {
        unimplemented!()
    }
    pub fn another_thing<S: SimpleTrait>(_s: S) -> Self {
        unimplemented!()
    }
}

pub static SOME_STRUCT: SomeStruct = SomeStruct;
pub const SOME_CONST: SomeStruct = SomeStruct;

pub mod some_pub_mod {
    use external_lib::SomeStruct;

    pub static OPTIONAL_STRUCT: Option<SomeStruct> = None;
    pub const OPTIONAL_CONST: Option<SomeStruct> = None;
}

pub type NotExternalReferencing = u32;
pub type ExternalReferencingTypedef = SomeStruct;
pub type OptionalExternalReferencingTypedef = Option<SomeStruct>;
pub type DynExternalReferencingTypedef = Box<dyn SimpleTrait>;

pub fn fn_with_external_trait_bounds<I, O, E, T>(_thing: T)
where
    I: Into<SomeStruct>,
    O: Into<SomeOtherStruct>,
    E: std::error::Error,
    T: AssociatedGenericTrait<Input = I, Output = O, Error = E>,
{
}

pub trait SomeTraitWithExternalDefaultTypes {
    type Thing: SimpleTrait;
    type OtherThing: AssociatedGenericTrait<
        Input = SomeStruct,
        Output = u32,
        Error = SomeOtherStruct,
    >;

    fn something(&self, input: Self::Thing) -> Self::OtherThing;
}

pub trait SomeTraitWithGenericAssociatedType {
    type MyGAT<T>
    where
        T: SimpleTrait;

    fn some_fn<T: SimpleTrait>(&self, thing: Self::MyGAT<T>);
}

pub struct AssocConstStruct;

impl AssocConstStruct {
    pub const SOME_CONST: u32 = 5;

    pub const OTHER_CONST: SimpleNewType = SimpleNewType(5);
}
