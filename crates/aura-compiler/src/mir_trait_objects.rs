//! Trait objects in MIR.
//!
//! Lowering and specialization read a trait object type as the checker
//! wrote it, `dyn Trait[args]`, so a method call finds the trait and a
//! specialized copy substitutes the trait's arguments. After them a trait
//! object needs nothing its type could say: it is one handle to a boxed
//! value of some type known only at run time, which is what a value of an
//! unspecialized type parameter is. `erase_trait_objects` therefore
//! replaces each trait object type with a type parameter named after it,
//! and every later pass and both backends handle it as they handle an
//! erased generic value:
//!
//! - a value stored in a place of the erased type is boxed with its own
//!   type, so the box says which implementation to run;
//! - a method call is a `TraitMember` call, which `mir::methods` resolves
//!   to the trait's implementations, tried against the value's run-time
//!   type;
//! - the interpreter reads the value itself.
//!
//! The erased name holds a space, so it is no type parameter a program can
//! declare, and it differs for each trait and each list of arguments, so a
//! union keeps its trait object members apart.

use std::collections::BTreeMap;

use super::{specialize, MirModule};
use crate::sema::{CallableType, FunctionParamContract, ReturnedViewType, Type};

/// The erased form of a trait object type's name: `dyn Trait[args]` with
/// each argument erased in turn.
fn erased_name(trait_name: &str, trait_args: &[Type]) -> String {
    if trait_args.is_empty() {
        return format!("dyn {trait_name}");
    }
    format!(
        "dyn {trait_name}[{}]",
        trait_args
            .iter()
            .map(|arg| erased(arg).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn erased_params(params: &[FunctionParamContract]) -> Vec<FunctionParamContract> {
    params
        .iter()
        .map(|param| FunctionParamContract {
            keyword_only: param.keyword_only,
            name: param.name.clone(),
            ty: erased(&param.ty),
            passing: param.passing,
            has_default: param.has_default,
        })
        .collect()
}

/// `ty` with every trait object type erased. A union is normalized again,
/// because its members are ordered by their keys.
pub(crate) fn erased(ty: &Type) -> Type {
    if !ty.names_trait_object() {
        return ty.clone();
    }
    if let Some((trait_name, trait_args)) = ty.as_trait_object() {
        return Type::TypeParam(erased_name(trait_name, trait_args));
    }
    match ty {
        Type::Union(union) => Type::normalize_union(
            union.members.iter().map(erased).collect(),
            &union.module_name,
            &BTreeMap::new(),
        )
        .expect("erasure keeps a union's members"),
        Type::Named(name, args) => Type::Named(name.clone(), args.iter().map(erased).collect()),
        Type::Tuple(elements) => Type::Tuple(elements.iter().map(erased).collect()),
        Type::Function {
            params,
            return_type,
        } => Type::Function {
            params: erased_params(params),
            return_type: Box::new(erased(return_type)),
        },
        Type::Closure {
            params,
            return_type,
            captures,
            call_kind,
        } => Type::Closure {
            params: Box::new(erased_params(params)),
            return_type: Box::new(erased(return_type)),
            captures: Box::new(
                captures
                    .iter()
                    .map(|capture| {
                        let mut capture = capture.clone();
                        capture.ty = erased(&capture.ty);
                        capture
                    })
                    .collect(),
            ),
            call_kind: *call_kind,
        },
        Type::Callable(callable) => Type::Callable(Box::new(CallableType {
            task: callable.task,
            call_kind: callable.call_kind,
            params: erased_params(&callable.params),
            return_type: erased(&callable.return_type),
        })),
        Type::ReturnedView(view) => Type::ReturnedView(Box::new(ReturnedViewType {
            mutable: view.mutable,
            pointee: erased(&view.pointee),
            origin: view.origin,
        })),
        Type::TypeParam(_) | Type::Module(_) | Type::Unit => ty.clone(),
    }
}

/// Erases every trait object type the module names.
pub(super) fn erase_trait_objects(module: &mut MirModule) {
    if !names_trait_object(module) {
        return;
    }
    for function in module.functions.iter_mut().chain(module.top_level.as_mut()) {
        *function = specialize::erase_function_trait_objects(function);
    }
    for class in &mut module.classes {
        for field in &mut class.fields {
            field.ty = erased(&field.ty);
        }
    }
    for layout in &mut module.enums {
        for variant in &mut layout.variants {
            for payload in &mut variant.payloads {
                *payload = erased(payload);
            }
        }
    }
    for trait_impl in &mut module.trait_impls {
        trait_impl.for_type = erased(&trait_impl.for_type);
        for arg in &mut trait_impl.trait_args {
            *arg = erased(arg);
        }
    }
    for constant in &mut module.constants {
        constant.ty = erased(&constant.ty);
    }
}

/// Whether the module holds a value of a type that names a trait object.
/// Such a value sits in a parameter, a local, a result, a field, an enum
/// payload, or a constant, so a module without one names no trait object
/// in any instruction either and is left as it is.
fn names_trait_object(module: &MirModule) -> bool {
    let function_types = module
        .functions
        .iter()
        .chain(module.top_level.as_ref())
        .flat_map(|function| {
            function
                .params
                .iter()
                .map(|param| &param.ty)
                .chain(function.local_types.iter().map(|local| &local.ty))
                .chain([&function.return_type])
        });
    let field_types = module
        .classes
        .iter()
        .flat_map(|class| class.fields.iter().map(|field| &field.ty));
    let payload_types = module
        .enums
        .iter()
        .flat_map(|layout| layout.variants.iter().flat_map(|variant| &variant.payloads));
    let constant_types = module.constants.iter().map(|constant| &constant.ty);
    function_types
        .chain(field_types)
        .chain(payload_types)
        .chain(constant_types)
        .any(Type::names_trait_object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ReceiverKind;
    use crate::sema::ClosureCallKind;

    const SHAPES: &str = "trait Shape:\n    def area(self) -> int64\n\nclass Square:\n    side: int64\n\nimpl Shape for Square:\n    def area(self) -> int64:\n        return self.side * self.side\n\n";

    fn lowered(body: &str) -> MirModule {
        let program = crate::check_source(&format!("{SHAPES}{body}")).expect("the program checks");
        crate::mir::lower_implicit(&program)
    }

    fn param(ty: Type) -> FunctionParamContract {
        FunctionParamContract {
            keyword_only: false,
            name: "value".to_string(),
            ty,
            passing: ReceiverKind::Borrow,
            has_default: false,
        }
    }

    #[test]
    fn a_trait_object_type_erases_to_a_parameter_named_after_it() {
        let shape = Type::trait_object("Shape", Vec::new());
        let erased_shape = Type::TypeParam("dyn Shape".to_string());
        assert_eq!(erased(&shape), erased_shape);
        // Each list of arguments is its own erased type, and an argument
        // that is a trait object is erased inside the name.
        let reader = Type::trait_object("io.Reader", vec![Type::named("int64"), shape.clone()]);
        assert_eq!(
            erased(&reader),
            Type::TypeParam("dyn io.Reader[int64, dyn Shape]".to_string())
        );

        let list = |element: Type| Type::Named("list".to_string(), vec![element]);
        assert_eq!(erased(&list(shape.clone())), list(erased_shape.clone()));
        assert_eq!(
            erased(&Type::Tuple(vec![Type::named("int64"), shape.clone()])),
            Type::Tuple(vec![Type::named("int64"), erased_shape.clone()])
        );
        let optional =
            Type::normalize_union(vec![shape.clone(), Type::Unit], "main", &BTreeMap::new())
                .expect("a union");
        let Type::Union(union) = erased(&optional) else {
            panic!("a union stays a union");
        };
        assert!(union.members.contains(&erased_shape));
        assert!(union.members.contains(&Type::Unit));

        assert_eq!(
            erased(&Type::Function {
                params: vec![param(shape.clone())],
                return_type: Box::new(shape.clone()),
            }),
            Type::Function {
                params: vec![param(erased_shape.clone())],
                return_type: Box::new(erased_shape.clone()),
            }
        );
        assert_eq!(
            erased(&Type::Closure {
                params: Box::new(vec![param(shape.clone())]),
                return_type: Box::new(shape.clone()),
                captures: Box::new(Vec::new()),
                call_kind: ClosureCallKind::Repeatable,
            }),
            Type::Closure {
                params: Box::new(vec![param(erased_shape.clone())]),
                return_type: Box::new(erased_shape.clone()),
                captures: Box::new(Vec::new()),
                call_kind: ClosureCallKind::Repeatable,
            }
        );
        assert_eq!(
            erased(&Type::Callable(Box::new(CallableType {
                task: false,
                call_kind: ClosureCallKind::Repeatable,
                params: vec![param(shape.clone())],
                return_type: shape.clone(),
            }))),
            Type::Callable(Box::new(CallableType {
                task: false,
                call_kind: ClosureCallKind::Repeatable,
                params: vec![param(erased_shape.clone())],
                return_type: erased_shape.clone(),
            }))
        );
        assert_eq!(
            erased(&Type::ReturnedView(Box::new(ReturnedViewType {
                mutable: true,
                pointee: shape,
                origin: 1,
            }))),
            Type::ReturnedView(Box::new(ReturnedViewType {
                mutable: true,
                pointee: erased_shape,
                origin: 1,
            }))
        );

        // A type that names no trait object is returned as it is.
        for plain in [
            Type::Unit,
            Type::TypeParam("T".to_string()),
            list(Type::named("str")),
        ] {
            assert_eq!(erased(&plain), plain);
        }
    }

    #[test]
    fn a_lowered_module_names_no_trait_object() {
        let module = lowered(
            "class Scene:\n    hero: dyn Shape\n    shapes: list[dyn Shape]\n\nenum Slot:\n    Full(dyn Shape)\n    Empty\n\nUNIT: dyn Shape = Square(side=1)\n\ndef total(shapes: list[dyn Shape]) -> int64:\n    mut sum = 0\n    for shape in shapes:\n        sum += shape.area()\n    return sum\n\ndef pick() -> dyn Shape:\n    return Square(side=2)\n\ndef main() -> None:\n    scene = Scene(hero=pick(), shapes=[Square(side=3)])\n    slot = Slot.Full(Square(side=4))\n    measure: def(dyn Shape) -> int64 = lambda shape: shape.area()\n    print(total(scene.shapes) + measure(scene.hero) + UNIT.area())\n",
        );
        assert!(!names_trait_object(&module));
        let erased_shape = Type::TypeParam("dyn Shape".to_string());
        let pick = module
            .functions
            .iter()
            .find(|function| function.name == "pick")
            .expect("`pick` is lowered");
        assert_eq!(pick.return_type, erased_shape);
        let scene = module
            .classes
            .iter()
            .find(|class| class.name == "Scene")
            .expect("`Scene` is lowered");
        assert_eq!(scene.fields[0].ty, erased_shape);
        assert!(module
            .enums
            .iter()
            .flat_map(|layout| layout.variants.iter().flat_map(|variant| &variant.payloads))
            .any(|payload| *payload == erased_shape));
        assert!(module
            .constants
            .iter()
            .any(|constant| constant.ty == erased_shape));
    }

    #[test]
    fn a_module_without_trait_objects_is_left_as_it_is() {
        let mut module = lowered(
            "def twice[T: Shape](item: T) -> int64:\n    return item.area() * 2\n\ndef main() -> None:\n    print(twice(Square(side=2)))\n",
        );
        assert!(!names_trait_object(&module));
        let before = format!("{module:?}");
        erase_trait_objects(&mut module);
        assert_eq!(format!("{module:?}"), before);
    }
}
