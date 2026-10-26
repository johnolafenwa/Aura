//! The layout plan of a module (typed-layout step 2a).
//!
//! Every type a module's functions, classes, enums, and constants use gets
//! a native layout: a scalar, an inline struct, a tagged union, a typed
//! handle, or a pointer to a heap block on a recursive edge. The plan lays
//! types out with `aura_core::layout`, the module the runtime's converters
//! use too, so both agree on every offset. A type's descriptor carries the
//! definitions its layout reaches, so the runtime lays it out again from the
//! descriptor alone.
//!
//! Code generation does not read the plan yet: values keep their boxed
//! representation until step 2c. See
//! `architecture_docs/24-typed-layouts-design-note.md`.

use std::collections::BTreeMap;

use aura_core::layout::{
    encode_descriptor, ClassDefinition, EnumDefinition, LayoutBuilder, LayoutEnv, LayoutId,
    LayoutTable,
};

use super::{MirFunction, MirModule};
use crate::sema::Type;

/// The layout of every type a module uses.
pub(crate) struct LayoutPlan {
    env: LayoutEnv,
    table: LayoutTable,
    /// Each type by its descriptor, so equal types share one entry.
    types: BTreeMap<String, (Type, LayoutId)>,
}

/// The classes and enums of `module`, as layouts read them.
pub(crate) fn layout_env(module: &MirModule) -> LayoutEnv {
    LayoutEnv {
        classes: module
            .classes
            .iter()
            .map(|class| {
                (
                    class.name.clone(),
                    ClassDefinition {
                        type_params: class.type_params.clone(),
                        fields: class
                            .fields
                            .iter()
                            .map(|field| (field.name.clone(), field.ty.clone()))
                            .collect(),
                    },
                )
            })
            .collect(),
        enums: module
            .enums
            .iter()
            .map(|enumeration| {
                (
                    enumeration.name.clone(),
                    EnumDefinition {
                        type_params: enumeration.type_params.clone(),
                        variants: enumeration
                            .variants
                            .iter()
                            .map(|variant| (variant.name.clone(), variant.payloads.clone()))
                            .collect(),
                    },
                )
            })
            .collect(),
    }
}

impl LayoutPlan {
    /// Lays out every type `module` uses: each function's parameters,
    /// locals, and result, each class field and enum payload, and each
    /// constant.
    pub(crate) fn for_module(module: &MirModule) -> Result<Self, String> {
        let env = layout_env(module);
        let mut used = Vec::new();
        for function in module.functions.iter().chain(&module.top_level) {
            function_types(function, &mut used);
        }
        for class in &module.classes {
            used.extend(class.fields.iter().map(|field| &field.ty));
        }
        for enumeration in &module.enums {
            used.extend(
                enumeration
                    .variants
                    .iter()
                    .flat_map(|variant| &variant.payloads),
            );
        }
        used.extend(module.constants.iter().map(|constant| &constant.ty));

        let mut builder = LayoutBuilder::new(&env);
        let mut types = BTreeMap::new();
        for ty in used {
            // A returned view is where a call's result points, not a value.
            if matches!(ty, Type::ReturnedView(_)) {
                continue;
            }
            let key = aura_core::type_descriptor::encode(ty);
            if types.contains_key(&key) {
                continue;
            }
            let id = builder
                .layout(ty)
                .map_err(|error| format!("type `{ty}` has no layout: {error}"))?;
            types.insert(key, (ty.clone(), id));
        }
        let table = builder.into_table();
        table.validate().map_err(|error| error.0)?;
        Ok(Self { env, table, types })
    }

    pub(crate) fn table(&self) -> &LayoutTable {
        &self.table
    }

    /// The layout of a type the module uses.
    pub(crate) fn layout_of(&self, ty: &Type) -> Option<LayoutId> {
        self.types
            .get(&aura_core::type_descriptor::encode(ty))
            .map(|(_, id)| *id)
    }

    /// Every type the module uses, with its layout.
    pub(crate) fn types(&self) -> impl Iterator<Item = (&Type, LayoutId)> {
        self.types.values().map(|(ty, id)| (ty, *id))
    }

    /// The descriptor the runtime lays `ty` out from.
    pub(crate) fn descriptor(&self, ty: &Type) -> String {
        encode_descriptor(ty, &self.env)
    }
}

fn function_types<'a>(function: &'a MirFunction, out: &mut Vec<&'a Type>) {
    out.extend(function.params.iter().map(|param| &param.ty));
    out.extend(function.local_types.iter().map(|local| &local.ty));
    out.push(&function.return_type);
}

#[cfg(test)]
#[path = "mir_layout_tests.rs"]
mod tests;
