/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::config::Config;
use crate::context::{ContextStack, ContextType};
use crate::error::{RefType, ValidationError};
use anyhow::{anyhow, Context as _, Result};
use rustdoc_types::{
    Crate, FnDecl, GenericArgs, GenericBound, GenericParamDef, GenericParamDefKind, Generics, Id,
    Item, ItemEnum, ItemSummary, Struct, Term, Trait, Type, Variant, Visibility, WherePredicate,
};
use smithy_rs_tool_common::macros::here;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use tracing::{debug, debug_span};

macro_rules! enter_debug_span {
    ($guard_name:ident, $name:expr, $($fields:tt)*) => {
        let _span = debug_span!($name, $($fields)*);
        let $guard_name = _span.enter();
    };
}

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

    fn visit_item(&self, context: &ContextStack, item: &Item) -> Result<()> {
        enter_debug_span!(_g, "visit_item", path = %context.to_string(), name = ?item.name, id = %item.id.0);

        if !Self::is_public(context, item) {
            return Ok(());
        }

        let mut context = context.clone();
        match &item.inner {
            ItemEnum::AssocConst { .. } => unimplemented!("visit_item ItemEnum::AssocConst"),
            ItemEnum::AssocType { bounds, default } => {
                context.push(ContextType::AssocType, item);
                if let Some(typ) = default {
                    self.visit_type(&context, &RefType::AssocType, typ)?;
                }
                self.visit_generic_bounds(&context, bounds)?;
            }
            ItemEnum::Constant(constant) => {
                context.push(ContextType::Constant, item);
                self.visit_type(&context, &RefType::Constant, &constant.type_)?;
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
                    self.check_external(&context, &RefType::ReExport, id)
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
                self.visit_type(&context, &RefType::Static, &sttc.type_)?;
            }
            ItemEnum::Struct(strct) => {
                context.push(ContextType::Struct, item);
                self.visit_struct(&context, strct)?;
            }
            ItemEnum::StructField(typ) => {
                context.push(ContextType::StructField, item);
                self.visit_type(&context, &RefType::StructField, typ)
                    .context(here!())?;
            }
            ItemEnum::Trait(trt) => {
                context.push(ContextType::Trait, item);
                self.visit_trait(&context, trt)?;
            }
            ItemEnum::Typedef(typedef) => {
                context.push(ContextType::TypeDef, item);
                self.visit_type(&context, &RefType::TypeDef, &typedef.type_)
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

    fn visit_trait(&self, context: &ContextStack, trt: &Trait) -> Result<()> {
        enter_debug_span!(_g, "visit_trait", path = %context.to_string());
        self.visit_generics(context, &trt.generics)?;
        self.visit_generic_bounds(context, &trt.bounds)?;
        for id in &trt.items {
            let item = self.item(id)?;
            self.visit_item(context, item)?;
        }
        Ok(())
    }

    fn visit_impl(&self, context: &ContextStack, item: &Item) -> Result<()> {
        enter_debug_span!(_g, "visit_impl", path = %context.to_string(), id = %item.id.0);

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
                self.visit_type(context, &RefType::ImplementedTrait, trait_)
                    .context(here!())?;
            }
        } else {
            unreachable!("should be passed an Impl item");
        }
        Ok(())
    }

    fn visit_fn_decl(&self, context: &ContextStack, decl: &FnDecl) -> Result<()> {
        enter_debug_span!(_g, "visit_fn_decl", path = %context.to_string());

        for (index, (name, typ)) in decl.inputs.iter().enumerate() {
            if index == 0 && name == "self" {
                continue;
            }
            self.visit_type(context, &RefType::ArgumentNamed(name.into()), typ)
                .context(here!())?;
        }
        if let Some(output) = &decl.output {
            self.visit_type(context, &RefType::ReturnValue, output)
                .context(here!())?;
        }
        Ok(())
    }

    fn visit_type(&self, context: &ContextStack, what: &RefType, typ: &Type) -> Result<()> {
        enter_debug_span!(_g, "visit_type", path = %context.to_string(), what = %what);

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
                    self.visit_type(context, &RefType::EnumTupleEntry, typ)?;
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
                self.visit_type(context, &RefType::QualifiedSelfType, self_type)?;
                self.visit_type(context, &RefType::QualifiedSelfTypeAsTrait, trait_)?;
            }
        }
        Ok(())
    }

    fn visit_generic_args(&self, context: &ContextStack, args: &GenericArgs) -> Result<()> {
        enter_debug_span!(_g, "visit_generic_args", path = %context.to_string());

        match args {
            GenericArgs::AngleBracketed { args, bindings } => {
                for arg in args {
                    match arg {
                        rustdoc_types::GenericArg::Type(typ) => {
                            self.visit_type(context, &RefType::GenericArg, typ)?
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
                                self.visit_type(context, &RefType::GenericDefaultBinding, typ)
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
                    self.visit_type(context, &RefType::ClosureInput, input)
                        .context(here!())?;
                }
                if let Some(output) = output {
                    self.visit_type(context, &RefType::ClosureOutput, output)
                        .context(here!())?;
                }
            }
        }
        Ok(())
    }

    fn visit_generic_bounds(&self, context: &ContextStack, bounds: &[GenericBound]) -> Result<()> {
        enter_debug_span!(_g, "visit_generic_bounds", path = %context.to_string());

        for bound in bounds {
            if let GenericBound::TraitBound {
                trait_,
                generic_params,
                ..
            } = bound
            {
                self.visit_type(context, &RefType::TraitBound, trait_)
                    .context(here!())?;
                for param_def in generic_params {
                    match &param_def.kind {
                        GenericParamDefKind::Type { bounds, default } => {
                            self.visit_generic_bounds(context, bounds)?;
                            if let Some(default) = default {
                                self.visit_type(context, &RefType::GenericDefaultBinding, default)
                                    .context(here!())?;
                            }
                        }
                        GenericParamDefKind::Const { ty, .. } => {
                            self.visit_type(context, &RefType::GenericDefaultBinding, ty)
                                .context(here!())?;
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    fn visit_generic_param_defs(
        &self,
        context: &ContextStack,
        params: &[GenericParamDef],
    ) -> Result<()> {
        enter_debug_span!(_g, "visit_generic_param_defs", path = %context.to_string());

        for param in params {
            match &param.kind {
                GenericParamDefKind::Type { bounds, default } => {
                    self.visit_generic_bounds(context, bounds)?;
                    if let Some(typ) = default {
                        self.visit_type(context, &RefType::GenericDefaultBinding, typ)
                            .context(here!())?;
                    }
                }
                GenericParamDefKind::Const { ty, .. } => {
                    self.visit_type(context, &RefType::ConstGeneric, ty)
                        .context(here!())?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn visit_generics(&self, context: &ContextStack, generics: &Generics) -> Result<()> {
        enter_debug_span!(_g, "visit_generics", path = %context.to_string());

        self.visit_generic_param_defs(context, &generics.params)?;
        for where_pred in &generics.where_predicates {
            match where_pred {
                WherePredicate::BoundPredicate { ty, bounds } => {
                    self.visit_type(context, &RefType::WhereBound, ty)
                        .context(here!())?;
                    self.visit_generic_bounds(context, bounds)?;
                }
                WherePredicate::RegionPredicate { bounds, .. } => {
                    self.visit_generic_bounds(context, bounds)?;
                }
                WherePredicate::EqPredicate { lhs, .. } => {
                    self.visit_type(context, &RefType::WhereBound, lhs)
                        .context(here!())?;
                }
            }
        }
        Ok(())
    }

    fn visit_variant(&self, context: &ContextStack, item: &Item, variant: &Variant) -> Result<()> {
        enter_debug_span!(_g, "visit_variant", path = %context.to_string(), id = %item.id.0);

        match variant {
            Variant::Plain => {}
            Variant::Tuple(types) => {
                for typ in types {
                    let mut variant_context = context.clone();
                    variant_context.push(ContextType::EnumVariant, item);
                    self.visit_type(context, &RefType::EnumTupleEntry, typ)?;
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

    fn check_external(&self, context: &ContextStack, what: &RefType, id: &Id) -> Result<()> {
        if let Some(crate_name) = self.crate_name(id) {
            let type_name = self.type_name(id)?;
            if !is_allowed_type(&self.config, &self.root_crate_name, crate_name, &type_name) {
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

    fn crate_name(&self, id: &Id) -> Option<&str> {
        self.item_summary(id)?.path.get(0).map(|s| s.as_str())
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

fn is_allowed_type(
    config: &Config,
    root_crate_name: &str,
    crate_name: &str,
    type_name: &str,
) -> bool {
    match crate_name {
        _ if crate_name == root_crate_name => true,
        "alloc" => config.allow_alloc,
        "core" => config.allow_core,
        "std" => config.allow_std,
        _ => config
            .allowed_external_types
            .iter()
            .any(|glob| glob.matches(type_name)),
    }
}

#[cfg(test)]
mod tests {
    use super::is_allowed_type;
    use crate::config::Config;
    use wildmatch::WildMatch;

    #[test]
    fn test_is_allowed_type() {
        let config = Config {
            allowed_external_types: vec![WildMatch::new("one::*"), WildMatch::new("two::*")],
            ..Default::default()
        };
        assert!(is_allowed_type(&config, "root", "alloc", "alloc::System"));
        assert!(is_allowed_type(&config, "root", "core", "std::vec::Vec"));
        assert!(is_allowed_type(&config, "root", "std", "std::path::Path"));

        assert!(is_allowed_type(&config, "root", "root", "root::thing"));
        assert!(is_allowed_type(&config, "root", "one", "one::thing"));
        assert!(is_allowed_type(&config, "root", "two", "two::thing"));
        assert!(!is_allowed_type(&config, "root", "three", "three::thing"));
    }
}
