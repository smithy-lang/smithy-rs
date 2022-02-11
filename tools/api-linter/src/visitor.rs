/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::config::Config;
use crate::context::{ContextStack, ContextType};
use crate::error::{ErrorLocation, ValidationError};
use anyhow::{anyhow, Context as _, Result};
use rustdoc_types::{
    Crate, FnDecl, GenericArgs, GenericBound, GenericParamDef, GenericParamDefKind, Generics, Id,
    Item, ItemEnum, ItemSummary, Struct, Term, Trait, Type, Variant, Visibility, WherePredicate,
};
use smithy_rs_tool_common::macros::here;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use tracing::debug;
use tracing_attributes::instrument;

/// Visits all items in the Rustdoc JSON output to discover external types in public APIs
/// and track them as validation errors if the [`Config`] doesn't allow them.
pub struct Visitor {
    config: Config,
    root_crate_id: u32,
    root_crate_name: String,
    index: HashMap<Id, Item>,
    paths: HashMap<Id, ItemSummary>,
    errors: RefCell<BTreeSet<ValidationError>>,
}

impl Visitor {
    pub fn new(config: Config, package: Crate) -> Result<Self> {
        Ok(Visitor {
            config,
            root_crate_id: Self::root_crate_id(&package)?,
            root_crate_name: Self::root_crate_name(&package)?,
            index: package.index,
            paths: package.paths,
            errors: RefCell::new(BTreeSet::new()),
        })
    }

    /// This is the entry point for visiting the entire Rustdoc JSON tree, starting
    /// from the root module (the only module where `is_crate` is true).
    pub fn visit_all(self) -> Result<BTreeSet<ValidationError>> {
        let root_context = ContextStack::new(&self.root_crate_name);
        let root_module = self
            .index
            .values()
            .filter_map(|item| {
                if let ItemEnum::Module(module) = &item.inner {
                    Some(module)
                } else {
                    None
                }
            })
            .find(|module| module.is_crate)
            .ok_or_else(|| anyhow!("failed to find crate root module"))?;

        for id in &root_module.items {
            let item = self.item(id).context(here!())?;
            self.visit_item(&root_context, item)?;
        }
        Ok(self.errors.take())
    }

    /// Returns true if the given item is public. In some cases, this must be determined
    /// by examining the surrounding context. For example, enum variants are public if the
    /// enum is public, even if their visibility is set to `Visibility::Default`.
    fn is_public(context: &ContextStack, item: &Item) -> bool {
        match item.visibility {
            Visibility::Public => true,
            Visibility::Default => match &item.inner {
                // Enum variants are public if the enum is public
                ItemEnum::Variant(_) => matches!(context.last_typ(), Some(ContextType::Enum)),
                // Struct fields inside of enum variants are public if the enum is public
                ItemEnum::StructField(_) => {
                    matches!(context.last_typ(), Some(ContextType::EnumVariant))
                }
                // Trait items are public if the trait is public
                _ => matches!(context.last_typ(), Some(ContextType::Trait)),
            },
            _ => false,
        }
    }

    #[instrument(level = "debug", skip(self, context, item), fields(path = %context.to_string(), name = ?item.name, id = %item.id.0))]
    fn visit_item(&self, context: &ContextStack, item: &Item) -> Result<()> {
        if !Self::is_public(context, item) {
            return Ok(());
        }

        let mut context = context.clone();
        match &item.inner {
            ItemEnum::AssocConst { .. } => unimplemented!("visit_item ItemEnum::AssocConst"),
            ItemEnum::AssocType { bounds, default } => {
                context.push(ContextType::AssocType, item);
                if let Some(typ) = default {
                    self.visit_type(&context, &ErrorLocation::AssocType, typ)?;
                }
                self.visit_generic_bounds(&context, bounds)?;
            }
            ItemEnum::Constant(constant) => {
                context.push(ContextType::Constant, item);
                self.visit_type(&context, &ErrorLocation::Constant, &constant.type_)?;
            }
            ItemEnum::Enum(enm) => {
                context.push(ContextType::Enum, item);
                self.visit_generics(&context, &enm.generics)?;
                for id in &enm.impls {
                    self.visit_impl(&context, self.item(id)?)?;
                }
                for id in &enm.variants {
                    self.visit_item(&context, self.item(id)?)?;
                }
            }
            ItemEnum::ForeignType => unimplemented!("visit_item ItemEnum::ForeignType"),
            ItemEnum::Function(function) => {
                context.push(ContextType::Function, item);
                self.visit_fn_decl(&context, &function.decl)?;
                self.visit_generics(&context, &function.generics)?;
            }
            ItemEnum::Import(import) => {
                if let Some(id) = &import.id {
                    context.push_raw(ContextType::ReExport, &import.name, item.span.as_ref());
                    self.check_external(&context, &ErrorLocation::ReExport, id)
                        .context(here!())?;
                }
            }
            ItemEnum::Method(method) => {
                context.push(ContextType::Method, item);
                self.visit_fn_decl(&context, &method.decl)?;
                self.visit_generics(&context, &method.generics)?;
            }
            ItemEnum::Module(module) => {
                if !module.is_crate {
                    context.push(ContextType::Module, item);
                }
                for id in &module.items {
                    let module_item = self.item(id)?;
                    // Re-exports show up twice in the doc json: once as an `ItemEnum::Import`,
                    // and once as the type as if it were originating from the root crate (but
                    // with a different crate ID). We only want to examine the `ItemEnum::Import`
                    // for re-exports since it includes the correct span where the re-export occurs,
                    // and we don't want to examine the innards of the re-export.
                    if module_item.crate_id == self.root_crate_id {
                        self.visit_item(&context, module_item)?;
                    }
                }
            }
            ItemEnum::OpaqueTy(_) => unimplemented!("visit_item ItemEnum::OpaqueTy"),
            ItemEnum::Static(sttc) => {
                context.push(ContextType::Static, item);
                self.visit_type(&context, &ErrorLocation::Static, &sttc.type_)?;
            }
            ItemEnum::Struct(strct) => {
                context.push(ContextType::Struct, item);
                self.visit_struct(&context, strct)?;
            }
            ItemEnum::StructField(typ) => {
                context.push(ContextType::StructField, item);
                self.visit_type(&context, &ErrorLocation::StructField, typ)
                    .context(here!())?;
            }
            ItemEnum::Trait(trt) => {
                context.push(ContextType::Trait, item);
                self.visit_trait(&context, trt)?;
            }
            ItemEnum::Typedef(typedef) => {
                context.push(ContextType::TypeDef, item);
                self.visit_type(&context, &ErrorLocation::TypeDef, &typedef.type_)
                    .context(here!())?;
                self.visit_generics(&context, &typedef.generics)?;
            }
            // Trait aliases aren't stable:
            // https://doc.rust-lang.org/beta/unstable-book/language-features/trait-alias.html
            ItemEnum::TraitAlias(_) => unimplemented!("unstable trait alias support"),
            ItemEnum::Union(_) => unimplemented!("union support"),
            ItemEnum::Variant(variant) => {
                context.push(ContextType::EnumVariant, item);
                self.visit_variant(&context, item, variant)?;
            }
            ItemEnum::ExternCrate { .. }
            | ItemEnum::Impl(_)
            | ItemEnum::Macro(_)
            | ItemEnum::PrimitiveType(_)
            | ItemEnum::ProcMacro(_) => {}
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, context, strct), fields(path = %context.to_string()))]
    fn visit_struct(&self, context: &ContextStack, strct: &Struct) -> Result<()> {
        self.visit_generics(context, &strct.generics)?;
        for id in &strct.fields {
            let field = self.item(id)?;
            self.visit_item(context, field)?;
        }
        for id in &strct.impls {
            self.visit_impl(context, self.item(id)?)?;
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, context, trt), fields(path = %context.to_string()))]
    fn visit_trait(&self, context: &ContextStack, trt: &Trait) -> Result<()> {
        self.visit_generics(context, &trt.generics)?;
        self.visit_generic_bounds(context, &trt.bounds)?;
        for id in &trt.items {
            let item = self.item(id)?;
            self.visit_item(context, item)?;
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, context, item), fields(path = %context.to_string(), id = %item.id.0))]
    fn visit_impl(&self, context: &ContextStack, item: &Item) -> Result<()> {
        if let ItemEnum::Impl(imp) = &item.inner {
            // Ignore blanket implementations
            if imp.blanket_impl.is_some() {
                return Ok(());
            }
            self.visit_generics(context, &imp.generics)?;
            for id in &imp.items {
                self.visit_item(context, self.item(id)?)?;
            }
            if let Some(trait_) = &imp.trait_ {
                self.visit_type(context, &ErrorLocation::ImplementedTrait, trait_)
                    .context(here!())?;
            }
        } else {
            unreachable!("should be passed an Impl item");
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, context, decl), fields(path = %context.to_string()))]
    fn visit_fn_decl(&self, context: &ContextStack, decl: &FnDecl) -> Result<()> {
        for (index, (name, typ)) in decl.inputs.iter().enumerate() {
            if index == 0 && name == "self" {
                continue;
            }
            self.visit_type(context, &ErrorLocation::ArgumentNamed(name.into()), typ)
                .context(here!())?;
        }
        if let Some(output) = &decl.output {
            self.visit_type(context, &ErrorLocation::ReturnValue, output)
                .context(here!())?;
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, context, typ), fields(path = %context.to_string()))]
    fn visit_type(&self, context: &ContextStack, what: &ErrorLocation, typ: &Type) -> Result<()> {
        match typ {
            Type::ResolvedPath {
                id,
                args,
                param_names,
                ..
            } => {
                self.check_external(context, what, id).context(here!())?;
                if let Some(args) = args {
                    self.visit_generic_args(context, args)?;
                }
                self.visit_generic_bounds(context, param_names)?;
            }
            Type::Generic(_) => {}
            Type::Primitive(_) => {}
            Type::FunctionPointer(fp) => {
                self.visit_fn_decl(context, &fp.decl)?;
                self.visit_generic_param_defs(context, &fp.generic_params)?;
            }
            Type::Tuple(types) => {
                for typ in types {
                    self.visit_type(context, &ErrorLocation::EnumTupleEntry, typ)?;
                }
            }
            Type::Slice(typ) => self.visit_type(context, what, typ).context(here!())?,
            Type::Array { type_, .. } => self.visit_type(context, what, type_).context(here!())?,
            Type::ImplTrait(impl_trait) => {
                for bound in impl_trait {
                    match bound {
                        GenericBound::TraitBound {
                            trait_,
                            generic_params,
                            ..
                        } => {
                            self.visit_type(context, what, trait_)?;
                            self.visit_generic_param_defs(context, generic_params)?;
                        }
                        GenericBound::Outlives(_) => {}
                    }
                }
            }
            Type::Infer => unimplemented!("visit_type Type::Infer"),
            Type::RawPointer { type_: _, .. } => unimplemented!("visit_type Type::RawPointer"),
            Type::BorrowedRef { type_, .. } => {
                self.visit_type(context, what, type_).context(here!())?
            }
            Type::QualifiedPath {
                self_type, trait_, ..
            } => {
                self.visit_type(context, &ErrorLocation::QualifiedSelfType, self_type)?;
                self.visit_type(context, &ErrorLocation::QualifiedSelfTypeAsTrait, trait_)?;
            }
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, context, args), fields(path = %context.to_string()))]
    fn visit_generic_args(&self, context: &ContextStack, args: &GenericArgs) -> Result<()> {
        match args {
            GenericArgs::AngleBracketed { args, bindings } => {
                for arg in args {
                    match arg {
                        rustdoc_types::GenericArg::Type(typ) => {
                            self.visit_type(context, &ErrorLocation::GenericArg, typ)?
                        }
                        rustdoc_types::GenericArg::Lifetime(_)
                        | rustdoc_types::GenericArg::Const(_)
                        | rustdoc_types::GenericArg::Infer => {}
                    }
                }
                for binding in bindings {
                    match &binding.binding {
                        rustdoc_types::TypeBindingKind::Equality(term) => {
                            if let Term::Type(typ) = term {
                                self.visit_type(
                                    context,
                                    &ErrorLocation::GenericDefaultBinding,
                                    typ,
                                )
                                .context(here!())?;
                            }
                        }
                        rustdoc_types::TypeBindingKind::Constraint(bounds) => {
                            self.visit_generic_bounds(context, bounds)?;
                        }
                    }
                }
            }
            GenericArgs::Parenthesized { inputs, output } => {
                for input in inputs {
                    self.visit_type(context, &ErrorLocation::ClosureInput, input)
                        .context(here!())?;
                }
                if let Some(output) = output {
                    self.visit_type(context, &ErrorLocation::ClosureOutput, output)
                        .context(here!())?;
                }
            }
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, context, bounds), fields(path = %context.to_string()))]
    fn visit_generic_bounds(&self, context: &ContextStack, bounds: &[GenericBound]) -> Result<()> {
        for bound in bounds {
            if let GenericBound::TraitBound {
                trait_,
                generic_params,
                ..
            } = bound
            {
                self.visit_type(context, &ErrorLocation::TraitBound, trait_)
                    .context(here!())?;
                for param_def in generic_params {
                    match &param_def.kind {
                        GenericParamDefKind::Type { bounds, default } => {
                            self.visit_generic_bounds(context, bounds)?;
                            if let Some(default) = default {
                                self.visit_type(
                                    context,
                                    &ErrorLocation::GenericDefaultBinding,
                                    default,
                                )
                                .context(here!())?;
                            }
                        }
                        GenericParamDefKind::Const { ty, .. } => {
                            self.visit_type(context, &ErrorLocation::GenericDefaultBinding, ty)
                                .context(here!())?;
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, context, params), fields(path = %context.to_string()))]
    fn visit_generic_param_defs(
        &self,
        context: &ContextStack,
        params: &[GenericParamDef],
    ) -> Result<()> {
        for param in params {
            match &param.kind {
                GenericParamDefKind::Type { bounds, default } => {
                    self.visit_generic_bounds(context, bounds)?;
                    if let Some(typ) = default {
                        self.visit_type(context, &ErrorLocation::GenericDefaultBinding, typ)
                            .context(here!())?;
                    }
                }
                GenericParamDefKind::Const { ty, .. } => {
                    self.visit_type(context, &ErrorLocation::ConstGeneric, ty)
                        .context(here!())?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, context, generics), fields(path = %context.to_string()))]
    fn visit_generics(&self, context: &ContextStack, generics: &Generics) -> Result<()> {
        self.visit_generic_param_defs(context, &generics.params)?;
        for where_pred in &generics.where_predicates {
            match where_pred {
                WherePredicate::BoundPredicate { ty, bounds } => {
                    self.visit_type(context, &ErrorLocation::WhereBound, ty)
                        .context(here!())?;
                    self.visit_generic_bounds(context, bounds)?;
                }
                WherePredicate::RegionPredicate { bounds, .. } => {
                    self.visit_generic_bounds(context, bounds)?;
                }
                WherePredicate::EqPredicate { lhs, .. } => {
                    self.visit_type(context, &ErrorLocation::WhereBound, lhs)
                        .context(here!())?;
                }
            }
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, context, item, variant), fields(path = %context.to_string(), id = %item.id.0))]
    fn visit_variant(&self, context: &ContextStack, item: &Item, variant: &Variant) -> Result<()> {
        match variant {
            Variant::Plain => {}
            Variant::Tuple(types) => {
                for typ in types {
                    let mut variant_context = context.clone();
                    variant_context.push(ContextType::EnumVariant, item);
                    self.visit_type(context, &ErrorLocation::EnumTupleEntry, typ)?;
                }
            }
            Variant::Struct(ids) => {
                for id in ids {
                    self.visit_item(context, self.item(id)?)?;
                }
            }
        }
        Ok(())
    }

    fn check_external(&self, context: &ContextStack, what: &ErrorLocation, id: &Id) -> Result<()> {
        if let Ok(type_name) = self.type_name(id) {
            if !self.config.allows_type(&self.root_crate_name, &type_name) {
                self.add_error(ValidationError::unapproved_external_type_ref(
                    self.type_name(id)?,
                    what,
                    context.to_string(),
                    context.last_span(),
                ));
            }
        }
        // Crates like `pin_project` do some shenanigans to create and reference types that don't end up
        // in the doc index, but that should only happen within the root crate.
        else if !id.0.starts_with(&format!("{}:", self.root_crate_id)) {
            unreachable!("A type is referencing another type that is not in the index, and that type is from another crate.");
        }
        Ok(())
    }

    fn add_error(&self, error: ValidationError) {
        debug!("detected error {:?}", error);
        self.errors.borrow_mut().insert(error);
    }

    fn item(&self, id: &Id) -> Result<&Item> {
        self.index
            .get(id)
            .ok_or_else(|| anyhow!("Failed to find item in index for ID {:?}", id))
            .context(here!())
    }

    fn item_summary(&self, id: &Id) -> Option<&ItemSummary> {
        self.paths.get(id)
    }

    fn type_name(&self, id: &Id) -> Result<String> {
        Ok(self.item_summary(id).context(here!())?.path.join("::"))
    }

    fn root_crate_id(package: &Crate) -> Result<u32> {
        Ok(Self::root(package)?.crate_id)
    }

    fn root_crate_name(package: &Crate) -> Result<String> {
        Ok(Self::root(package)?
            .name
            .as_ref()
            .expect("root should always have a name")
            .clone())
    }

    fn root(package: &Crate) -> Result<&Item> {
        package
            .index
            .get(&package.root)
            .ok_or_else(|| anyhow!("root not found in index"))
            .context(here!())
    }
}
