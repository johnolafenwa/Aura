use super::{validate_methods, MethodsConsumer};
use crate::mir::{
    CallTarget, CleanupCallee, Instruction, MethodCandidate, MethodDispatch, MirModule, Rvalue,
};
use crate::runtime_ops::{ResourceOp, RuntimeOp};
use crate::sema::Type;

const METHODS: &str = r#"
trait From[T]:
    def from(value: own T) -> Self

trait Speak:
    def speak(self) -> str

class Dog:
    legs: int64

    def name(self) -> str:
        return "dog"

class Cat:
    lives: int64

impl Speak for Dog:
    def speak(self) -> str:
        return "woof"

impl Speak for Cat:
    def speak(self) -> str:
        return "meow"

class Resource:
    open: bool

    def close(mut self) -> None:
        self.open = false

    def reopen(mut self) -> None:
        self.open = true

enum ReadError:
    Missing

enum AppError:
    Read(ReadError)

impl From[ReadError] for AppError:
    def from(value: own ReadError) -> AppError:
        return AppError.Read(value)

def read(flag: bool) -> Result[int32, ReadError]:
    if flag:
        return Result.Ok(7)
    return Result.Err(ReadError.Missing)

def load(flag: bool) -> Result[int32, AppError]:
    value = try read(flag)
    return Result.Ok(value + 1)

def show[T: Speak](value: T) -> str:
    return value.speak()

def talk(pet: Dog | Cat) -> str:
    return pet.speak()

def main():
    dog = Dog(legs=4)
    print(dog.name())
    print(dog.speak())
    print(talk(Cat(lives=9)))
    print(show(dog))
    print(load(true))
    with resource = Resource(open=true):
        print(resource.open)
"#;

fn instructions_mut<'a>(
    module: &'a mut MirModule,
    function: &'a str,
) -> impl Iterator<Item = &'a mut Instruction> {
    module
        .functions
        .iter_mut()
        .filter(move |candidate| candidate.name == function)
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.instructions)
}

/// The dispatch of each call to `.field` that `function` makes.
fn dispatches<'a>(
    module: &'a mut MirModule,
    function: &'a str,
    field: &str,
) -> Vec<&'a mut Option<MethodDispatch>> {
    instructions_mut(module, function)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Member {
                                field: called,
                                dispatch,
                                ..
                            }
                            | CallTarget::TraitMember {
                                field: called,
                                dispatch,
                                ..
                            },
                        ..
                    },
                ..
            } if called == field => Some(dispatch),
            _ => None,
        })
        .collect()
}

fn try_froms<'a>(module: &'a mut MirModule, function: &'a str) -> Vec<&'a mut Option<String>> {
    instructions_mut(module, function)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::Try { from, .. },
                ..
            } => Some(from),
            _ => None,
        })
        .collect()
}

fn cleanup_callees<'a>(
    module: &'a mut MirModule,
    function: &'a str,
) -> Vec<&'a mut Option<CleanupCallee>> {
    instructions_mut(module, function)
        .filter_map(|instruction| match instruction {
            Instruction::PushCleanup { callee, .. } => Some(callee),
            _ => None,
        })
        .collect()
}

fn assert_boundaries_reject(module: &MirModule, expected: &str) {
    let runtime = crate::run_mir(module).expect_err("the interpreter must reject the module");
    assert!(runtime.message.contains(expected), "{}", runtime.message);
    let direct = crate::native_codegen::emit_host_object(module)
        .expect_err("the direct backend must reject the module");
    assert!(direct.contains(expected), "{direct}");
}

fn static_call(function: &str) -> Option<MethodDispatch> {
    Some(MethodDispatch::Static {
        function: function.to_string(),
    })
}

#[test]
fn lowering_names_what_each_member_call_try_and_cleanup_runs() {
    let mut module = crate::lower_source_to_mir(METHODS).expect("source should lower");
    assert_eq!(
        dispatches(&mut module, "main", "name"),
        [&mut static_call("Dog.name")]
    );
    let speak = dispatches(&mut module, "main", "speak");
    let [Some(MethodDispatch::Static { function })] = speak.as_slice() else {
        panic!("`dog.speak()` calls one method: {speak:?}");
    };
    assert!(function.contains("speak"), "{function}");
    let dog_speak = function.clone();

    let talk = dispatches(&mut module, "talk", "speak");
    let [Some(MethodDispatch::Union(arms))] = talk.as_slice() else {
        panic!("a union receiver switches on its members: {talk:?}");
    };
    assert_eq!(arms.len(), 2);
    assert!(arms
        .iter()
        .all(|arm| matches!(arm, MethodDispatch::Static { .. })));

    let show = dispatches(&mut module, "show", "speak");
    let [Some(MethodDispatch::Candidates { candidates, erased })] = show.as_slice() else {
        panic!("an erased receiver picks a candidate at run time: {show:?}");
    };
    assert!(*erased);
    assert_eq!(candidates.len(), 2);
    assert!(candidates.contains(&MethodCandidate {
        ty: Type::named("Dog"),
        function: dog_speak,
    }));

    let froms = try_froms(&mut module, "load");
    let [Some(from)] = froms.as_slice() else {
        panic!("`try` names its conversion: {froms:?}");
    };
    assert!(from.contains("from"), "{from}");
    assert_eq!(
        cleanup_callees(&mut module, "main"),
        [&mut Some(CleanupCallee::Method {
            function: "Resource.close".to_string()
        })]
    );
    assert_eq!(validate_methods(&module, MethodsConsumer::Native), Ok(()));

    // `lower_implicit` leaves them out; the native boundary names the same
    // callees.
    let program = crate::check_source(METHODS).expect("source should check");
    let mut implicit = crate::mir::lower_implicit(&program);
    assert_eq!(dispatches(&mut implicit, "main", "name"), [&mut None]);
    let unnamed = validate_methods(&implicit, MethodsConsumer::Native)
        .expect_err("an implicit module names no callees");
    assert!(unnamed.contains("names no"), "{unnamed}");
    crate::mir::validate_at_boundary(&mut implicit).expect("the module should validate");
    assert_eq!(
        dispatches(&mut implicit, "main", "name"),
        [&mut static_call("Dog.name")]
    );
}

#[test]
fn boundaries_reject_a_member_call_that_names_another_method() {
    let lowered = crate::lower_source_to_mir(METHODS).expect("source should lower");
    let mut wrong = lowered.clone();
    *dispatches(&mut wrong, "main", "name")[0] = static_call("Resource.reopen");
    assert_boundaries_reject(
        &wrong,
        "invalid MIR member call `.name` in `main` names a dispatch its receiver does not resolve to",
    );

    let mut arm = lowered.clone();
    if let Some(MethodDispatch::Union(arms)) = dispatches(&mut arm, "talk", "speak").remove(0) {
        arms.swap(0, 1);
    }
    assert_boundaries_reject(
        &arm,
        "invalid MIR member call `.speak` in `talk` names a dispatch",
    );

    let mut candidates = lowered;
    if let Some(MethodDispatch::Candidates { erased, .. }) =
        dispatches(&mut candidates, "show", "speak").remove(0)
    {
        *erased = false;
    }
    assert_boundaries_reject(
        &candidates,
        "invalid MIR member call `.speak` in `show` names a dispatch",
    );
}

#[test]
fn boundaries_reject_a_missing_dispatch() {
    let mut missing = crate::lower_source_to_mir(METHODS).expect("source should lower");
    *dispatches(&mut missing, "main", "name")[0] = None;
    assert_boundaries_reject(
        &missing,
        "invalid MIR member call `.name` in `main` names no dispatch",
    );
}

#[test]
fn boundaries_reject_a_cleanup_that_names_a_callee_other_than_its_exit() {
    let lowered = crate::lower_source_to_mir(METHODS).expect("source should lower");
    let mut wrong = lowered.clone();
    *cleanup_callees(&mut wrong, "main")[0] = Some(CleanupCallee::Method {
        function: "Resource.reopen".to_string(),
    });
    assert_boundaries_reject(
        &wrong,
        "invalid MIR cleanup `resource` in `main` names a callee that is not its exit",
    );

    let mut runtime = lowered.clone();
    *cleanup_callees(&mut runtime, "main")[0] = Some(CleanupCallee::Runtime(RuntimeOp::Resource(
        ResourceOp::FileClose,
    )));
    assert_boundaries_reject(&runtime, "names a callee that is not its exit");

    let mut missing = lowered;
    *cleanup_callees(&mut missing, "main")[0] = None;
    assert_boundaries_reject(
        &missing,
        "invalid MIR cleanup `resource` in `main` names no callee",
    );
}

#[test]
fn boundaries_reject_a_try_that_names_the_wrong_conversion() {
    let lowered = crate::lower_source_to_mir(METHODS).expect("source should lower");
    let mut wrong = lowered.clone();
    *try_froms(&mut wrong, "load")[0] = Some("Dog.name".to_string());
    assert_boundaries_reject(
        &wrong,
        "invalid MIR `try` in `load` names a `From` conversion its error types do not resolve to",
    );
    let mut missing = lowered;
    *try_froms(&mut missing, "load")[0] = None;
    assert_boundaries_reject(
        &missing,
        "invalid MIR `try` in `load` names no `From` conversion",
    );
}

#[test]
fn resource_cleanups_name_their_runtime_close() {
    let source = r#"
def main():
    with group = TaskGroup():
        print(1)
"#;
    let mut module = crate::lower_source_to_mir(source).expect("source should lower");
    assert_eq!(
        cleanup_callees(&mut module, "main"),
        [&mut Some(CleanupCallee::CloseTaskGroup)]
    );
}

#[test]
fn the_interpreter_accepts_an_unnamed_call_that_resolves_to_nothing() {
    // A generic body may call a method no type of the program declares. The
    // interpreter dispatches on the runtime value, so it runs the module; the
    // native boundary refuses it.
    let source = r#"
trait Speak:
    def speak(self) -> str

def show[T: Speak](value: T) -> str:
    return value.speak()

def main():
    print(1)
"#;
    let module = crate::lower_source_to_mir(source).expect("source should lower");
    assert_eq!(
        validate_methods(&module, MethodsConsumer::Interpreter),
        Ok(())
    );
    let refused = validate_methods(&module, MethodsConsumer::Native)
        .expect_err("the native boundary needs a dispatch");
    assert!(
        refused.contains("`.speak` in `show` resolves to no method"),
        "{refused}"
    );
}

/// What the resolver makes of `receiver.field()` in the `METHODS` program.
fn resolve_call(
    receiver: &Type,
    field: &str,
    trait_name: Option<&str>,
    writes_back: bool,
) -> Result<MethodDispatch, String> {
    let module = crate::lower_source_to_mir(METHODS).expect("source should lower");
    let context = crate::mir::MirLoanValidationContext::new(&module).expect("context should build");
    let resolver = super::Resolver { context: &context };
    resolver.dispatch(
        receiver,
        &super::MemberCall {
            field,
            trait_name,
            args: &[],
            writes_back,
        },
    )
}

#[test]
fn opaque_receivers_dispatch_on_every_type_that_declares_the_method() {
    let widget = Type::named("Widget");
    assert_eq!(
        resolve_call(&widget, "speak", None, false),
        Ok(MethodDispatch::Candidates {
            candidates: vec![
                MethodCandidate {
                    ty: Type::named("Dog"),
                    function: "Speak for Dog.speak".to_string(),
                },
                MethodCandidate {
                    ty: Type::named("Cat"),
                    function: "Speak for Cat.speak".to_string(),
                },
            ],
            erased: false,
        })
    );
    assert_eq!(
        resolve_call(&widget, "name", None, false),
        Ok(MethodDispatch::Static {
            function: "Dog.name".to_string(),
        })
    );
    assert_eq!(
        resolve_call(&widget, "fly", None, false),
        Err("no type declares `.fly` for `Widget`".to_string())
    );
    assert_eq!(
        resolve_call(&Type::named("net.HttpResponse"), "close", None, false),
        Ok(MethodDispatch::Nothing)
    );
}

#[test]
fn the_resolver_refuses_calls_no_method_answers() {
    let callable = Type::Function {
        params: Vec::new(),
        return_type: Box::new(Type::Unit),
    };
    assert_eq!(
        resolve_call(&callable, "speak", None, false),
        Err("`.speak` is called on a callable value".to_string())
    );
    assert_eq!(
        resolve_call(&Type::named("Resource"), "close", None, false),
        Err("`Resource.close` takes `mut self`, and its receiver is a temporary".to_string())
    );
    assert_eq!(
        resolve_call(&Type::named("Dog"), "fly", None, false),
        Err("`Dog` has no method `fly`".to_string())
    );
}

#[test]
fn boundaries_reject_a_cleanup_whose_place_has_no_exit() {
    let lowered = crate::lower_source_to_mir(METHODS).expect("source should lower");
    let mut no_close = lowered.clone();
    no_close
        .classes
        .iter_mut()
        .find(|class| class.name == "Resource")
        .expect("the program declares `Resource`")
        .methods
        .retain(|method| method.name != "close");
    let error = validate_methods(&no_close, MethodsConsumer::Native)
        .expect_err("a class without `close` has no exit");
    assert!(
        error.contains(
            "invalid MIR cleanup `resource` in `main` resolves to no exit: class `Resource` has no `close` method"
        ),
        "{error}"
    );

    for (ty, expected) in [
        (Type::named("int64"), "`int64` is not a class or a resource"),
        (
            Type::Tuple(vec![Type::named("int64")]),
            "`(int64,)` is not a class or a resource",
        ),
    ] {
        let mut retyped = lowered.clone();
        for local in retyped
            .functions
            .iter_mut()
            .filter(|function| function.name == "main")
            .flat_map(|function| &mut function.local_types)
            .filter(|local| local.name == "resource")
        {
            local.ty = ty.clone();
        }
        let error = validate_methods(&retyped, MethodsConsumer::Native)
            .expect_err("the cleanup place has no exit");
        assert!(error.contains(expected), "{error}");
    }
}
