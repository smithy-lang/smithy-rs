/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::config::Config;
use crate::error::{ErrorLocation, ValidationError};
use crate::here;
use crate::path::{ComponentType, Path};
use anyhow::{anyhow, Context, Result};
use rustdoc_types::{
    Crate, FnDecl, GenericArgs, GenericBound, GenericParamDef, GenericParamDefKind, Generics, Id,
    Item, ItemEnum, ItemSummary, Struct, Term, Trait, Type, Variant, Visibility, WherePredicate,
};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use tracing::debug;
use tracing_attributes::instrument;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum VisibilityCheck {
    /// Check to make sure the item is public before visiting it
    Default,
    /// Assume the item is public and examine it.
    /// This is useful for visiting private items that are publically re-exported
    AssumePublic,
}

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
        let root_path = Path::new(&self.root_crate_name);
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
            self.visit_item(&root_path, item, VisibilityCheck::Default)?;
        }
        Ok(self.errors.take())
    }

    /// Returns true if the given item is public. In some cases, this must be determined
    /// by examining the surrounding context. For example, enum variants are public if the
    /// enum is public, even if their visibility is set to `Visibility::Default`.
    fn is_public(path: &Path, item: &Item) -> bool {
        match item.visibility {
            Visibility::Public => true,
            Visibility::Default => match &item.inner {
                // Enum variants are public if the enum is public
                ItemEnum::Variant(_) => matches!(path.last_typ(), Some(ComponentType::Enum)),
                // Struct fields inside of enum variants are public if the enum is public
                ItemEnum::StructField(_) => {
                    matches!(path.last_typ(), Some(ComponentType::EnumVariant))
                }
                // Trait items are public if the trait is public
                _ => matches!(path.last_typ(), Some(ComponentType::Trait)),
            },
            _ => false,
        }
    }

    #[instrument(level = "debug", skip(self, path, item), fields(path = %path, name = ?item.name, id = %item.id.0))]
    fn visit_item(
        &self,
        path: &Path,
        item: &Item,
        visibility_check: VisibilityCheck,
    ) -> Result<()> {
        if visibility_check == VisibilityCheck::Default && !Self::is_public(path, item) {
            return Ok(());
        }

        let mut path = path.clone();
        match &item.inner {
            ItemEnum::AssocConst { type_, .. } => {
                path.push(ComponentType::AssocConst, item);
                self.visit_type(&path, &ErrorLocation::StructField, type_)
                    .context(here!())?;
            }
            ItemEnum::AssocType {
                bounds,
                default,
                generics,
            } => {
                path.push(ComponentType::AssocType, item);
                if let Some(typ) = default {
                    self.visit_type(&path, &ErrorLocation::AssocType, typ)?;
                }
                self.visit_generic_bounds(&path, bounds)?;
                self.visit_generics(&path, generics)?;
            }
            ItemEnum::Constant(constant) => {
                path.push(ComponentType::Constant, item);
                self.visit_type(&path, &ErrorLocation::Constant, &constant.type_)?;
            }
            ItemEnum::Enum(enm) => {
                path.push(ComponentType::Enum, item);
                self.visit_generics(&path, &enm.generics)?;
                for id in &enm.impls {
                    self.visit_impl(&path, self.item(id)?)?;
                }
                for id in &enm.variants {
                    self.visit_item(&path, self.item(id)?, VisibilityCheck::Default)?;
                }
            }
            ItemEnum::ForeignType => unimplemented!("visit_item ItemEnum::ForeignType"),
            ItemEnum::Function(function) => {
                path.push(ComponentType::Function, item);
                self.visit_fn_decl(&path, &function.decl)?;
                self.visit_generics(&path, &function.generics)?;
            }
            ItemEnum::Import(import) => {
                if let Some(target_id) = &import.id {
                    if let Ok(target_item) = self.item(target_id) {
                        // Override the visibility check for re-exported items
                        self.visit_item(&path, target_item, VisibilityCheck::AssumePublic)
                            .context(here!())?;
                    }
                    path.push_raw(ComponentType::ReExport, &import.name, item.span.as_ref());
                    self.check_external(&path, &ErrorLocation::ReExport, target_id)
                        .context(here!())?;
                }
            }
            ItemEnum::Method(method) => {
                path.push(ComponentType::Method, item);
                self.visit_fn_decl(&path, &method.decl)?;
                self.visit_generics(&path, &method.generics)?;
            }
            ItemEnum::Module(module) => {
                if !module.is_crate {
                    path.push(ComponentType::Module, item);
                }
                for id in &module.items {
                    let module_item = self.item(id)?;
                    // Re-exports show up twice in the doc json: once as an `ItemEnum::Import`,
                    // and once as the type as if it were originating from the root crate (but
                    // with a different crate ID). We only want to examine the `ItemEnum::Import`
                    // for re-exports since it includes the correct span where the re-export occurs,
                    // and we don't want to examine the innards of the re-export.
                    if module_item.crate_id == self.root_crate_id {
                        self.visit_item(&path, module_item, VisibilityCheck::Default)?;
                    }
                }
            }
            ItemEnum::OpaqueTy(_) => unimplemented!("visit_item ItemEnum::OpaqueTy"),
            ItemEnum::Static(sttc) => {
                path.push(ComponentType::Static, item);
                self.visit_type(&path, &ErrorLocation::Static, &sttc.type_)?;
            }
            ItemEnum::Struct(strct) => {
                path.push(ComponentType::Struct, item);
                self.visit_struct(&path, strct)?;
            }
            ItemEnum::StructField(typ) => {
                path.push(ComponentType::StructField, item);
                self.visit_type(&path, &ErrorLocation::StructField, typ)
                    .context(here!())?;
            }
            ItemEnum::Trait(trt) => {
                path.push(ComponentType::Trait, item);
                self.visit_trait(&path, trt)?;
            }
            ItemEnum::Typedef(typedef) => {
                path.push(ComponentType::TypeDef, item);
                self.visit_type(&path, &ErrorLocation::TypeDef, &typedef.type_)
                    .context(here!())?;
                self.visit_generics(&path, &typedef.generics)?;
            }
            // Trait aliases aren't stable:
            // https://doc.rust-lang.org/beta/unstable-book/language-features/trait-alias.html
            ItemEnum::TraitAlias(_) => unimplemented!("unstable trait alias support"),
            ItemEnum::Union(_) => unimplemented!("union support"),
            ItemEnum::Variant(variant) => {
                path.push(ComponentType::EnumVariant, item);
                self.visit_variant(&path, variant)?;
            }
            ItemEnum::ExternCrate { .. }
            | ItemEnum::Impl(_)
            | ItemEnum::Macro(_)
            | ItemEnum::PrimitiveType(_)
            | ItemEnum::ProcMacro(_) => {}
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, path, strct), fields(path = %path))]
    fn visit_struct(&self, path: &Path, strct: &Struct) -> Result<()> {
        self.visit_generics(path, &strct.generics)?;
        for id in &strct.fields {
            let field = self.item(id)?;
            self.visit_item(path, field, VisibilityCheck::Default)?;
        }
        for id in &strct.impls {
            self.visit_impl(path, self.item(id)?)?;
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, path, trt), fields(path = %path))]
    fn visit_trait(&self, path: &Path, trt: &Trait) -> Result<()> {
        self.visit_generics(path, &trt.generics)?;
        self.visit_generic_bounds(path, &trt.bounds)?;
        for id in &trt.items {
            let item = self.item(id)?;
            self.visit_item(path, item, VisibilityCheck::Default)?;
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, path, item), fields(path = %path, id = %item.id.0))]
    fn visit_impl(&self, path: &Path, item: &Item) -> Result<()> {
        if let ItemEnum::Impl(imp) = &item.inner {
            // Ignore blanket implementations
            if imp.blanket_impl.is_some() {
                return Ok(());
            }
            self.visit_generics(path, &imp.generics)?;
            for id in &imp.items {
                self.visit_item(path, self.item(id)?, VisibilityCheck::Default)?;
            }
            if let Some(trait_) = &imp.trait_ {
                self.visit_type(path, &ErrorLocation::ImplementedTrait, trait_)
                    .context(here!())?;
            }
        } else {
            unreachable!("should be passed an Impl item");
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, path, decl), fields(path = %path))]
    fn visit_fn_decl(&self, path: &Path, decl: &FnDecl) -> Result<()> {
        for (index, (name, typ)) in decl.inputs.iter().enumerate() {
            if index == 0 && name == "self" {
                continue;
            }
            self.visit_type(path, &ErrorLocation::ArgumentNamed(name.into()), typ)
                .context(here!())?;
        }
        if let Some(output) = &decl.output {
            self.visit_type(path, &ErrorLocation::ReturnValue, output)
                .context(here!())?;
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, path, typ), fields(path = %path))]
    fn visit_type(&self, path: &Path, what: &ErrorLocation, typ: &Type) -> Result<()> {
        match typ {
            Type::ResolvedPath {
                id,
                args,
                param_names,
                ..
            } => {
                self.check_external(path, what, id).context(here!())?;
                if let Some(args) = args {
                    self.visit_generic_args(path, args)?;
                }
                self.visit_generic_bounds(path, param_names)?;
            }
            Type::Generic(_) => {}
            Type::Primitive(_) => {}
            Type::FunctionPointer(fp) => {
                self.visit_fn_decl(path, &fp.decl)?;
                self.visit_generic_param_defs(path, &fp.generic_params)?;
            }
            Type::Tuple(types) => {
                for typ in types {
                    self.visit_type(path, &ErrorLocation::EnumTupleEntry, typ)?;
                }
            }
            Type::Slice(typ) => self.visit_type(path, what, typ).context(here!())?,
            Type::Array { type_, .. } => self.visit_type(path, what, type_).context(here!())?,
            Type::ImplTrait(impl_trait) => {
                for bound in impl_trait {
                    match bound {
                        GenericBound::TraitBound {
                            trait_,
                            generic_params,
                            ..
                        } => {
                            self.visit_type(path, what, trait_)?;
                            self.visit_generic_param_defs(path, generic_params)?;
                        }
                        GenericBound::Outlives(_) => {}
                    }
                }
            }
            Type::Infer => unimplemented!("visit_type Type::Infer"),
            Type::RawPointer { type_: _, .. } => unimplemented!("visit_type Type::RawPointer"),
            Type::BorrowedRef { type_, .. } => {
                self.visit_type(path, what, type_).context(here!())?
            }
            Type::QualifiedPath {
                self_type, trait_, ..
            } => {
                self.visit_type(path, &ErrorLocation::QualifiedSelfType, self_type)?;
                self.visit_type(path, &ErrorLocation::QualifiedSelfTypeAsTrait, trait_)?;
            }
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, path, args), fields(path = %path))]
    fn visit_generic_args(&self, path: &Path, args: &GenericArgs) -> Result<()> {
        match args {
            GenericArgs::AngleBracketed { args, bindings } => {
                for arg in args {
                    match arg {
                        rustdoc_types::GenericArg::Type(typ) => {
                            self.visit_type(path, &ErrorLocation::GenericArg, typ)?
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
                                self.visit_type(path, &ErrorLocation::GenericDefaultBinding, typ)
                                    .context(here!())?;
                            }
                        }
                        rustdoc_types::TypeBindingKind::Constraint(bounds) => {
                            self.visit_generic_bounds(path, bounds)?;
                        }
                    }
                }
            }
            GenericArgs::Parenthesized { inputs, output } => {
                for input in inputs {
                    self.visit_type(path, &ErrorLocation::ClosureInput, input)
                        .context(here!())?;
                }
                if let Some(output) = output {
                    self.visit_type(path, &ErrorLocation::ClosureOutput, output)
                        .context(here!())?;
                }
            }
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, path, bounds), fields(path = %path))]
    fn visit_generic_bounds(&self, path: &Path, bounds: &[GenericBound]) -> Result<()> {
        for bound in bounds {
            if let GenericBound::TraitBound {
                trait_,
                generic_params,
                ..
            } = bound
            {
                self.visit_type(path, &ErrorLocation::TraitBound, trait_)
                    .context(here!())?;
                self.visit_generic_param_defs(path, generic_params)?;
            }
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, path, params), fields(path = %path))]
    fn visit_generic_param_defs(&self, path: &Path, params: &[GenericParamDef]) -> Result<()> {
        for param in params {
            match &param.kind {
                GenericParamDefKind::Type {
                    bounds,
                    default,
                    synthetic: _,
                } => {
                    self.visit_generic_bounds(path, bounds)?;
                    if let Some(typ) = default {
                        self.visit_type(path, &ErrorLocation::GenericDefaultBinding, typ)
                            .context(here!())?;
                    }
                }
                GenericParamDefKind::Const { type_, .. } => {
                    self.visit_type(path, &ErrorLocation::ConstGeneric, type_)
                        .context(here!())?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, path, generics), fields(path = %path))]
    fn visit_generics(&self, path: &Path, generics: &Generics) -> Result<()> {
        self.visit_generic_param_defs(path, &generics.params)?;
        for where_pred in &generics.where_predicates {
            match where_pred {
                WherePredicate::BoundPredicate {
                    type_,
                    bounds,
                    generic_params,
                } => {
                    self.visit_type(path, &ErrorLocation::WhereBound, type_)
                        .context(here!())?;
                    self.visit_generic_bounds(path, bounds)?;
                    self.visit_generic_param_defs(path, generic_params)?;
                }
                WherePredicate::RegionPredicate { bounds, .. } => {
                    self.visit_generic_bounds(path, bounds)?;
                }
                WherePredicate::EqPredicate { lhs, .. } => {
                    self.visit_type(path, &ErrorLocation::WhereBound, lhs)
                        .context(here!())?;
                }
            }
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(self, path, variant), fields(path = %path))]
    fn visit_variant(&self, path: &Path, variant: &Variant) -> Result<()> {
        match variant {
            Variant::Plain => {}
            Variant::Tuple(types) => {
                for typ in types {
                    self.visit_type(path, &ErrorLocation::EnumTupleEntry, typ)?;
                }
            }
            Variant::Struct(ids) => {
                for id in ids {
                    self.visit_item(path, self.item(id)?, VisibilityCheck::Default)?;
                }
            }
        }
        Ok(())
    }

    fn check_external(&self, path: &Path, what: &ErrorLocation, id: &Id) -> Result<()> {
        if let Ok(type_name) = self.type_name(id) {
            if !self.config.allows_type(&self.root_crate_name, &type_name) {
                self.add_error(ValidationError::unapproved_external_type_ref(
                    self.type_name(id)?,
                    what,
                    path.to_string(),
                    path.last_span(),
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
