//! Keeps source owners with observable destruction alive to their lexical
//! end. The ordinary ownership pass still releases compiler temporaries.

use super::*;

pub(super) fn program_has_hooks(program: &Program) -> bool {
    program
        .classes
        .values()
        .chain(
            program
                .module_registry
                .values()
                .chain(program.imported_modules.values())
                .flat_map(|m| m.all_classes.values()),
        )
        .any(|class| class.methods.contains_key(crate::sema::DROP_HOOK))
}

impl Lowerer<'_> {
    pub(super) fn arm_class_drop(&mut self, place: &str, class: &crate::sema::ClassInfo) {
        if class.methods.contains_key(crate::sema::DROP_HOOK) {
            self.emit(Instruction::ArmDrop {
                place: place.to_string(),
                function: mir_class_method_name(self.program, class, crate::sema::DROP_HOOK),
            });
        }
    }

    fn owners_from(&self, depth: usize) -> Vec<String> {
        if !self.has_class_hooks {
            return vec![];
        }
        self.binding_blocks
            .iter()
            .skip(depth)
            .rev()
            .flat_map(|block| block.owners.iter().rev())
            .filter(|place| {
                self.local_types
                    .get(*place)
                    .is_some_and(|ty| self.type_needs_owner(ty, &mut BTreeSet::new()))
            })
            .cloned()
            .collect()
    }

    pub(super) fn has_live_drop_owners(&self) -> bool {
        !self.owners_from(0).is_empty()
    }

    pub(super) fn end_owners_from(&mut self, depth: usize) {
        for place in self.owners_from(depth) {
            self.emit(Instruction::EndOwner { place });
        }
    }

    fn type_needs_owner(&self, ty: &Type, seen: &mut BTreeSet<String>) -> bool {
        if ty.is_trait_object() {
            return true;
        }
        match ty {
            Type::TypeParam(_) | Type::Callable(_) | Type::Closure { .. } => true,
            Type::Named(name, args) => {
                if args.iter().any(|arg| self.type_needs_owner(arg, seen)) {
                    return true;
                }
                if !seen.insert(name.clone()) {
                    return false;
                }
                self.resolve_class_info(name).is_some_and(|class| {
                    class.methods.contains_key(crate::sema::DROP_HOOK)
                        || class
                            .fields
                            .values()
                            .any(|field| self.type_needs_owner(&field.ty, seen))
                }) || self.resolve_enum_info(name).is_some_and(|enumeration| {
                    enumeration
                        .variants
                        .values()
                        .flat_map(|v| &v.payloads)
                        .any(|p| self.type_needs_owner(&p.ty, seen))
                })
            }
            Type::Tuple(types) => types.iter().any(|ty| self.type_needs_owner(ty, seen)),
            Type::Union(union) => union
                .members
                .iter()
                .any(|ty| self.type_needs_owner(ty, seen)),
            _ => false,
        }
    }
}
