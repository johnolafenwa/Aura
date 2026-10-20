//! Derived `json.Codec` implementations (ADR-0062).
//!
//! `impl json.Codec for Point` with no body asks the compiler for `encode`,
//! `decode`, and `schema`. Before checking, this pass writes those methods as
//! ordinary Aura source: each impl method delegates to a private module
//! function, and each distinct field type gets one encoder, decoder, and
//! schema helper. The generated source is parsed with its lines shifted far
//! past any real line, so its spans never collide with the module's own
//! span-keyed checker tables. A checker error inside generated code is
//! reported at the `impl` line instead.

use super::{BTreeMap, BTreeSet, Diagnostic, ImportedBinding, Item, Module, ModuleContext, Result};
use crate::ast::{ClassDecl, ImplDecl, Stmt, TypeRef, TypeRefKind};
use crate::diag::Span;

/// First line number of generated source.
pub(crate) const GENERATED_LINE_OFFSET: usize = 1_000_000_000;

/// Module binding the generated source uses for the `json` module, whatever
/// name the user imported it under.
const JSON: &str = "__aura_json";

const CODE: &str = "AU2046";

/// Where each derived implementation's generated source sits, so a checker
/// error inside it can be reported at the `impl`.
#[derive(Default)]
pub(super) struct DerivedSources {
    regions: Vec<Region>,
}

struct Region {
    lines: std::ops::Range<usize>,
    class: String,
    span: Span,
}

impl DerivedSources {
    pub(super) fn map_error(&self, mut diagnostic: Diagnostic) -> Diagnostic {
        let Some(span) = diagnostic.span else {
            return diagnostic;
        };
        if span.line < GENERATED_LINE_OFFSET {
            return diagnostic;
        }
        let region = self
            .regions
            .iter()
            .find(|region| region.lines.contains(&span.line))
            .or_else(|| self.regions.first());
        if let Some(region) = region {
            diagnostic.span = Some(region.span);
            diagnostic.message = format!(
                "derived `json.Codec` for `{}`: {}",
                region.class, diagnostic.message
            );
        }
        diagnostic
    }
}

/// Expands every bodyless `impl` in `module`. Returns the generated-source
/// regions for error mapping.
pub(super) fn expand(module: &mut Module, context: &mut ModuleContext) -> Result<DerivedSources> {
    let derived = module
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            Item::Impl(decl) if decl.derived => Some(index),
            _ => None,
        })
        .collect::<Vec<_>>();
    if derived.is_empty() {
        return Ok(DerivedSources::default());
    }

    let mut targets = Vec::new();
    for &index in &derived {
        let Item::Impl(decl) = &module.items[index] else {
            continue;
        };
        if !names_json_codec(&decl.trait_name, context) {
            return Err(Diagnostic::coded_at(
                CODE,
                decl.span,
                format!(
                    "only `json.Codec` can be derived; `impl {} for ...` needs a body",
                    decl.trait_name
                ),
            )
            .with_help("write the trait's methods in an indented block after `:`"));
        }
        let class = derived_class(module, decl)?;
        targets.push((index, class.name.clone(), decl.span));
    }

    let namespace = crate::builtin_sources::importable_builtin_namespace(&["json".to_string()])
        .ok_or_else(|| Diagnostic::coded_at(CODE, targets[0].2, "cannot load the `json` module"))?;
    context
        .imported_bindings
        .insert(JSON.to_string(), ImportedBinding::Module(namespace));

    let mut generator = Generator::new(module, context);
    for (_, class, span) in &targets {
        generator.derive_class(class, *span)?;
    }
    let Generator {
        functions,
        impls,
        defaults,
        ..
    } = generator;

    let mut source = String::new();
    let mut regions = Vec::new();
    let mut line = GENERATED_LINE_OFFSET;
    for chunk in impls.iter().chain(functions.iter()) {
        let start = line;
        source.push_str(&chunk.text);
        source.push_str("\n\n");
        line += chunk.text.lines().count() + 2;
        regions.push(Region {
            lines: start..line,
            class: chunk.owner.clone(),
            span: chunk.span,
        });
    }
    let generated =
        crate::parser::parse_generated(&source, GENERATED_LINE_OFFSET - 1).map_err(|error| {
            DerivedSources {
                regions: std::mem::take(&mut regions),
            }
            .map_error(error)
        })?;

    let mut generated_impls = BTreeMap::new();
    let mut generated_items = Vec::new();
    for item in generated.items {
        match item {
            Item::Impl(decl) => {
                generated_impls.insert(type_name(&decl.for_type), decl.methods);
            }
            Item::Function(mut function) => {
                if let Some(default) = defaults.get(&function.name) {
                    if let Some(Stmt::Return(stmt)) = function.body.first_mut() {
                        stmt.value = Some(default.clone());
                    }
                }
                generated_items.push(Item::Function(function));
            }
            other => generated_items.push(other),
        }
    }
    for (index, class, _) in &targets {
        if let Item::Impl(decl) = &mut module.items[*index] {
            decl.methods = generated_impls.remove(class).unwrap_or_default();
        }
    }
    module.items.extend(generated_items);
    Ok(DerivedSources { regions })
}

fn type_name(ty: &TypeRef) -> String {
    match &ty.kind {
        TypeRefKind::Named { name, .. } => name.clone(),
        _ => String::new(),
    }
}

/// Whether `written` names the `json.Codec` trait through this module's
/// imports: `json.Codec`, an aliased `j.Codec`, or a from-imported `Codec`.
fn names_json_codec(written: &str, context: &ModuleContext) -> bool {
    match written.rsplit_once('.') {
        Some((module_path, "Codec")) => module_binding_is_json(module_path, context),
        Some(_) => false,
        None => matches!(
            context.imported_bindings.get(written),
            Some(ImportedBinding::Trait(info))
                if info.module_name == "json" && info.decl.name == "Codec"
        ),
    }
}

fn module_binding_is_json(path: &str, context: &ModuleContext) -> bool {
    !path.contains('.')
        && matches!(
            context.imported_bindings.get(path),
            Some(ImportedBinding::Module(namespace)) if namespace.path == "json"
        )
}

fn derived_class<'a>(module: &'a Module, decl: &ImplDecl) -> Result<&'a ClassDecl> {
    let TypeRefKind::Named { name, args } = &decl.for_type.kind else {
        return Err(Diagnostic::coded_at(
            CODE,
            decl.span,
            "a derived `json.Codec` needs a class declared in this module",
        ));
    };
    let reject = |message: String| Err(Diagnostic::coded_at(CODE, decl.span, message));
    for item in &module.items {
        match item {
            Item::Class(class) if &class.name == name => {
                if !class.type_params.is_empty() || !args.is_empty() || !decl.type_params.is_empty()
                {
                    return reject(format!(
                        "a derived `json.Codec` for the generic class `{name}` is not implemented"
                    ));
                }
                if class.methods.iter().any(|method| method.name == "__init__") {
                    return Err(Diagnostic::coded_at(
                        CODE,
                        decl.span,
                        format!(
                            "cannot derive `json.Codec` for `{name}` because it declares `__init__`"
                        ),
                    )
                    .with_help(
                        "decode a plain wire class and call the initializer in an explicit implementation",
                    ));
                }
                return Ok(class);
            }
            Item::Enum(decl_enum) if &decl_enum.name == name => {
                return reject(format!(
                    "a derived `json.Codec` for the enum `{name}` is not implemented"
                ));
            }
            _ => {}
        }
    }
    reject(format!(
        "a derived `json.Codec` must be in the module that declares the class `{name}`"
    ))
}

/// The JSON shape of one field type.
#[derive(Clone)]
enum Shape {
    Bool,
    /// A fixed-width integer narrower than `int64`, with its range.
    Int {
        name: &'static str,
        low: i64,
        high: i64,
    },
    Int64,
    Float64,
    Float32,
    Str,
    Value,
    List(Box<Shape>),
    Dict(Box<Shape>),
    Tuple(Vec<Shape>),
    Optional(Box<Shape>),
    /// A class of this module with a derived implementation.
    Derived(String),
    /// Any other named type, which must implement `json.Codec`.
    Codec(String),
}

const NARROW_INTEGERS: &[(&str, i64, i64)] = &[
    ("int8", -128, 127),
    ("int16", -32_768, 32_767),
    ("int32", -2_147_483_648, 2_147_483_647),
    ("uint8", 0, 255),
    ("uint16", 0, 65_535),
    ("uint32", 0, 4_294_967_295),
];

impl Shape {
    fn aura_type(&self) -> String {
        match self {
            Shape::Bool => "bool".to_string(),
            Shape::Int { name, .. } => name.to_string(),
            Shape::Int64 => "int64".to_string(),
            Shape::Float64 => "float64".to_string(),
            Shape::Float32 => "float32".to_string(),
            Shape::Str => "str".to_string(),
            Shape::Value => format!("{JSON}.Value"),
            Shape::List(item) => format!("list[{}]", item.aura_type()),
            Shape::Dict(item) => format!("dict[str, {}]", item.aura_type()),
            Shape::Tuple(items) if items.len() == 1 => format!("({},)", items[0].aura_type()),
            Shape::Tuple(items) => format!(
                "({})",
                items
                    .iter()
                    .map(Shape::aura_type)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Shape::Optional(item) => format!("{} | None", item.aura_type()),
            Shape::Derived(name) | Shape::Codec(name) => name.clone(),
        }
    }
}

struct Chunk {
    text: String,
    owner: String,
    span: Span,
}

struct Generator<'a> {
    module: &'a Module,
    context: &'a ModuleContext,
    /// Classes whose implementation this pass derives.
    derived: BTreeSet<String>,
    /// Helper index for each Aura type text.
    helpers: BTreeMap<String, usize>,
    /// Classes whose decoder, encoder, and object schema already exist.
    classes_done: BTreeSet<String>,
    functions: Vec<Chunk>,
    impls: Vec<Chunk>,
    /// Field default helpers, by function name, with the default expression.
    defaults: BTreeMap<String, crate::ast::Expr>,
    owner: String,
    span: Span,
}

impl<'a> Generator<'a> {
    fn new(module: &'a Module, context: &'a ModuleContext) -> Self {
        let derived = module
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Impl(decl) if decl.derived => Some(type_name(&decl.for_type)),
                _ => None,
            })
            .collect();
        Self {
            module,
            context,
            derived,
            helpers: BTreeMap::new(),
            classes_done: BTreeSet::new(),
            functions: Vec::new(),
            impls: Vec::new(),
            defaults: BTreeMap::new(),
            owner: String::new(),
            span: Span::new(0, 0),
        }
    }

    fn class(&self, name: &str) -> Option<&'a ClassDecl> {
        self.module.items.iter().find_map(|item| match item {
            Item::Class(class) if class.name == name => Some(class),
            _ => None,
        })
    }

    fn emit(&mut self, text: String) {
        self.functions.push(Chunk {
            text,
            owner: self.owner.clone(),
            span: self.span,
        });
    }

    fn derive_class(&mut self, name: &str, span: Span) -> Result<()> {
        self.owner = name.to_string();
        self.span = span;
        let index = self.helper(&Shape::Derived(name.to_string()))?;
        let trait_name = self
            .module
            .items
            .iter()
            .find_map(|item| match item {
                Item::Impl(decl) if decl.derived && type_name(&decl.for_type) == name => {
                    Some(decl.trait_name.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| format!("{JSON}.Codec"));
        self.impls.push(Chunk {
            text: format!(
                "impl {trait_name} for {name}:\n\
                 \x20   def encode(self) -> {JSON}.Value:\n\
                 \x20       return __aura_json_encode_{index}(self)\n\n\
                 \x20   def decode(value: own {JSON}.Value) -> Result[{name}, {JSON}.DecodeError]:\n\
                 \x20       return __aura_json_decode_{index}(value)\n\n\
                 \x20   def schema() -> {JSON}.Value:\n\
                 \x20       return __aura_json_root_schema_{index}()"
            ),
            owner: name.to_string(),
            span,
        });
        self.emit(format!(
            "def __aura_json_root_schema_{index}() -> {JSON}.Value:\n\
             \x20   mut defs: dict[str, {JSON}.Value] = {{}}\n\
             \x20   body = __aura_json_object_{index}(defs)\n\
             \x20   return {JSON}.__with_defs(body, defs)"
        ));
        Ok(())
    }

    /// Classifies a field type, or rejects it with the field's name.
    fn shape(&self, ty: &TypeRef, class: &str, field: &str, depth: usize) -> Result<Shape> {
        let reject = |detail: String| {
            Err(Diagnostic::coded_at(
                CODE,
                ty.span,
                format!("cannot derive `json.Codec` for `{class}`: field `{field}` {detail}"),
            ))
        };
        if depth > 64 {
            return reject("has a type nested too deeply".to_string());
        }
        match &ty.kind {
            TypeRefKind::Named { name, args } => {
                self.named_shape(ty, name, args, class, field, depth)
            }
            TypeRefKind::Tuple(items) => Ok(Shape::Tuple(
                items
                    .iter()
                    .map(|item| self.shape(item, class, field, depth + 1))
                    .collect::<Result<_>>()?,
            )),
            TypeRefKind::Union(members) => {
                let (none, other): (Vec<_>, Vec<_>) = members.iter().partition(|member| {
                    matches!(&member.kind, TypeRefKind::Named { name, args } if name == "None" && args.is_empty())
                });
                if none.len() == 1 && other.len() == 1 {
                    return Ok(Shape::Optional(Box::new(self.shape(
                        other[0],
                        class,
                        field,
                        depth + 1,
                    )?)));
                }
                reject(
                    "has a union type, and a derived codec supports only `T | None` unions"
                        .to_string(),
                )
            }
            TypeRefKind::Callable { .. } | TypeRefKind::Function { .. } => {
                reject("has a callable type, which has no JSON form".to_string())
            }
        }
    }

    fn named_shape(
        &self,
        ty: &TypeRef,
        name: &str,
        args: &[TypeRef],
        class: &str,
        field: &str,
        depth: usize,
    ) -> Result<Shape> {
        let reject = |detail: String| {
            Err(Diagnostic::coded_at(
                CODE,
                ty.span,
                format!("cannot derive `json.Codec` for `{class}`: field `{field}` {detail}"),
            ))
        };
        let scalar = match name {
            "bool" => Some(Shape::Bool),
            "int" | "int64" => Some(Shape::Int64),
            "float64" => Some(Shape::Float64),
            "float32" => Some(Shape::Float32),
            "str" => Some(Shape::Str),
            _ => NARROW_INTEGERS
                .iter()
                .find(|(integer, _, _)| *integer == name)
                .map(|&(name, low, high)| Shape::Int { name, low, high }),
        };
        if let Some(scalar) = scalar {
            if !args.is_empty() {
                return reject(format!("has the invalid type `{name}[...]`"));
            }
            return Ok(scalar);
        }
        match (name, args) {
            ("list", [item]) => {
                return Ok(Shape::List(Box::new(self.shape(
                    item,
                    class,
                    field,
                    depth + 1,
                )?)))
            }
            ("dict", [key, item]) => {
                if !matches!(&key.kind, TypeRefKind::Named { name, args } if name == "str" && args.is_empty())
                {
                    return reject(
                        "has a `dict` type whose keys are not `str`, and JSON object keys are strings"
                            .to_string(),
                    );
                }
                return Ok(Shape::Dict(Box::new(self.shape(
                    item,
                    class,
                    field,
                    depth + 1,
                )?)));
            }
            ("uint64" | "uintsize" | "intsize" | "int128" | "uint128", _) => {
                return reject(format!(
                    "has the type `{name}`, and a derived codec supports integers up to `int64` and `uint32`"
                ));
            }
            (
                "list" | "dict" | "set" | "Array" | "bytes" | "Result" | "Lookup" | "Task" | "None",
                _,
            ) => {
                return reject(format!(
                    "has the type `{}`, which a derived `json.Codec` cannot encode",
                    render(ty)
                ));
            }
            _ => {}
        }
        if self.is_json_value(name) {
            return Ok(Shape::Value);
        }
        if !name.contains('.') {
            if let Some(target) = self.local_alias(name) {
                if !args.is_empty() {
                    return reject(format!(
                        "uses the generic alias `{name}`, which a derived codec does not expand"
                    ));
                }
                return self.shape(target, class, field, depth + 1);
            }
        }
        if !args.is_empty() {
            return reject(format!(
                "has the generic type `{}`, which a derived `json.Codec` does not support",
                render(ty)
            ));
        }
        if self.derived.contains(name) {
            return Ok(Shape::Derived(name.to_string()));
        }
        Ok(Shape::Codec(name.to_string()))
    }

    fn is_json_value(&self, name: &str) -> bool {
        match name.rsplit_once('.') {
            Some((path, "Value")) => module_binding_is_json(path, self.context),
            Some(_) => false,
            None => matches!(
                self.context.imported_bindings.get(name),
                Some(ImportedBinding::Enum(info))
                    if info.module_name == "json" && info.decl.name == "Value"
            ),
        }
    }

    fn local_alias(&self, name: &str) -> Option<&'a TypeRef> {
        self.module.items.iter().find_map(|item| match item {
            Item::TypeAlias(alias) if alias.name == name && alias.type_params.is_empty() => {
                Some(&alias.target)
            }
            _ => None,
        })
    }

    /// Returns the helper index for `shape`, generating its encoder,
    /// decoder, and schema the first time.
    fn helper(&mut self, shape: &Shape) -> Result<usize> {
        let key = shape.aura_type();
        if let Some(&index) = self.helpers.get(&key) {
            return Ok(index);
        }
        let index = self.helpers.len();
        self.helpers.insert(key.clone(), index);
        let ty = key;
        let value = format!("{JSON}.Value");
        let error = format!("{JSON}.DecodeError");
        let (encode, decode, schema) = match shape {
            Shape::Bool => (
                format!("return {value}.Bool(value)"),
                format!("return {JSON}.__decode_bool(value)"),
                format!("return {JSON}.__type_schema(\"boolean\")"),
            ),
            Shape::Int { name, low, high } => (
                format!("return {value}.Int(value as int64)"),
                format!("return {JSON}.__decode_{name}(value)"),
                format!("return {JSON}.__integer_schema({low}, {high})"),
            ),
            Shape::Int64 => (
                format!("return {value}.Int(value)"),
                format!("return {JSON}.__decode_int64(value)"),
                format!("return {JSON}.__type_schema(\"integer\")"),
            ),
            Shape::Float64 => (
                format!("return {value}.Float(value)"),
                format!("return {JSON}.__decode_float64(value)"),
                format!("return {JSON}.__type_schema(\"number\")"),
            ),
            Shape::Float32 => (
                format!("return {value}.Float(value as float64)"),
                format!("return {JSON}.__decode_float32(value)"),
                format!("return {JSON}.__type_schema(\"number\")"),
            ),
            Shape::Str => (
                format!("return {value}.String(value.clone())"),
                format!("return {JSON}.__decode_str(value)"),
                format!("return {JSON}.__type_schema(\"string\")"),
            ),
            Shape::Value => (
                format!("return {JSON}.__copy(value)"),
                "return Result.Ok(value)".to_string(),
                format!("return {value}.Object({{}})"),
            ),
            Shape::List(item) => {
                let item_ty = item.aura_type();
                let item = self.helper(item)?;
                (
                    format!(
                        "mut items: list[{value}] = []\n\
                         for item in value:\n\
                         \x20   items.append(__aura_json_encode_{item}(item))\n\
                         return {value}.Array(items)"
                    ),
                    format!(
                        "match own {JSON}.__array(value):\n\
                         \x20   case Result.Err(error):\n\
                         \x20       return Result.Err(error)\n\
                         \x20   case Result.Ok(items):\n\
                         \x20       mut result: list[{item_ty}] = []\n\
                         \x20       mut index = 0\n\
                         \x20       for item in own items:\n\
                         \x20           match own __aura_json_decode_{item}(item):\n\
                         \x20               case Result.Ok(decoded):\n\
                         \x20                   result.append(decoded)\n\
                         \x20               case Result.Err(error):\n\
                         \x20                   return Result.Err({JSON}.__at(error, f\"[{{index}}]\"))\n\
                         \x20           index += 1\n\
                         \x20       return Result.Ok(result)"
                    ),
                    format!(
                        "items = __aura_json_schema_{item}(defs)\n\
                         return {value}.Object({{\"type\": {value}.String(\"array\"), \"items\": items}})"
                    ),
                )
            }
            Shape::Dict(item) => {
                let item_ty = item.aura_type();
                let item = self.helper(item)?;
                (
                    format!(
                        "mut fields: dict[str, {value}] = {{}}\n\
                         for key in value.keys():\n\
                         \x20   view item = value[key]\n\
                         \x20   fields[key.clone()] = __aura_json_encode_{item}(item)\n\
                         return {value}.Object(fields)"
                    ),
                    format!(
                        "match own {JSON}.__entries(value):\n\
                         \x20   case Result.Err(error):\n\
                         \x20       return Result.Err(error)\n\
                         \x20   case Result.Ok(entries):\n\
                         \x20       mut result: dict[str, {item_ty}] = {{}}\n\
                         \x20       for key, item in own entries:\n\
                         \x20           match own __aura_json_decode_{item}(item):\n\
                         \x20               case Result.Ok(decoded):\n\
                         \x20                   result[key] = decoded\n\
                         \x20               case Result.Err(error):\n\
                         \x20                   return Result.Err({JSON}.__at(error, \".\" + key))\n\
                         \x20       return Result.Ok(result)"
                    ),
                    format!(
                        "items = __aura_json_schema_{item}(defs)\n\
                         return {value}.Object({{\"type\": {value}.String(\"object\"), \"additionalProperties\": items}})"
                    ),
                )
            }
            Shape::Tuple(items) => {
                let mut indexes = Vec::new();
                for item in items {
                    indexes.push((self.helper(item)?, item.aura_type()));
                }
                let names = (0..items.len())
                    .map(|position| format!("e{position}"))
                    .collect::<Vec<_>>();
                let pattern = if names.len() == 1 {
                    format!("({},)", names[0])
                } else {
                    format!("({})", names.join(", "))
                };
                let encoded = indexes
                    .iter()
                    .zip(&names)
                    .map(|((index, _), name)| format!("__aura_json_encode_{index}({name})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut decode = format!(
                    "match own {JSON}.__tuple_items(value, {count}):\n\
                     \x20   case Result.Err(error):\n\
                     \x20       return Result.Err(error)\n\
                     \x20   case Result.Ok(items):\n",
                    count = items.len()
                );
                for (position, (index, item_ty)) in indexes.iter().enumerate() {
                    decode.push_str(&format!(
                        "\x20       mut e{position}: list[{item_ty}] = []\n\
                         \x20       match own __aura_json_decode_{index}(items.pop(0)):\n\
                         \x20           case Result.Ok(decoded):\n\
                         \x20               e{position}.append(decoded)\n\
                         \x20           case Result.Err(error):\n\
                         \x20               return Result.Err({JSON}.__at(error, \"[{position}]\"))\n"
                    ));
                }
                let popped = names
                    .iter()
                    .map(|name| format!("{name}.pop()"))
                    .collect::<Vec<_>>();
                let built = if popped.len() == 1 {
                    format!("({},)", popped[0])
                } else {
                    format!("({})", popped.join(", "))
                };
                decode.push_str(&format!("\x20       return Result.Ok({built})"));
                let mut schema = format!("mut prefix: list[{value}] = []\n");
                for (index, _) in &indexes {
                    schema.push_str(&format!(
                        "prefix.append(__aura_json_schema_{index}(defs))\n"
                    ));
                }
                schema.push_str(&format!(
                    "return {value}.Object(\n\
                     \x20   {{\n\
                     \x20       \"type\": {value}.String(\"array\"),\n\
                     \x20       \"prefixItems\": {value}.Array(prefix),\n\
                     \x20       \"items\": {value}.Bool(false),\n\
                     \x20       \"minItems\": {value}.Int({count}),\n\
                     \x20   }}\n\
                     )",
                    count = items.len()
                ));
                (
                    format!(
                        "match value:\n\
                         \x20   case {pattern}:\n\
                         \x20       return {value}.Array([{encoded}])"
                    ),
                    decode,
                    schema,
                )
            }
            Shape::Optional(item) => {
                let item = self.helper(item)?;
                (
                    format!(
                        "if value is None:\n\
                         \x20   return {value}.Null\n\
                         return __aura_json_encode_{item}(value)"
                    ),
                    format!(
                        "if {JSON}.__is_null(value):\n\
                         \x20   return Result.Ok(None)\n\
                         match own __aura_json_decode_{item}(value):\n\
                         \x20   case Result.Ok(decoded):\n\
                         \x20       return Result.Ok(decoded)\n\
                         \x20   case Result.Err(error):\n\
                         \x20       return Result.Err(error)"
                    ),
                    format!(
                        "item = __aura_json_schema_{item}(defs)\n\
                         return {value}.Object({{\"anyOf\": {value}.Array([item, {JSON}.__type_schema(\"null\")])}})"
                    ),
                )
            }
            Shape::Derived(name) => {
                self.derived_class_helpers(name, index)?;
                (
                    format!("return __aura_json_encode_class_{index}(value)"),
                    format!("return __aura_json_decode_class_{index}(value)"),
                    format!(
                        "if not (\"{name}\" in defs):\n\
                         \x20   defs[\"{name}\"] = {value}.Null\n\
                         \x20   body = __aura_json_object_{index}(defs)\n\
                         \x20   defs[\"{name}\"] = body\n\
                         return {JSON}.__ref(\"{name}\")"
                    ),
                )
            }
            Shape::Codec(name) => {
                let leaf = name.rsplit('.').next().unwrap_or(name);
                (
                    format!("return {JSON}.encode(value)"),
                    format!("return {JSON}.decode[{name}](value)"),
                    format!("return {JSON}.__lift(defs, \"{leaf}\", {JSON}.schema[{name}]())"),
                )
            }
        };
        self.emit(format!(
            "def __aura_json_encode_{index}(value: {ty}) -> {value}:\n{}",
            indent(&encode)
        ));
        self.emit(format!(
            "def __aura_json_decode_{index}(value: own {value}) -> Result[{ty}, {error}]:\n{}",
            indent(&decode)
        ));
        self.emit(format!(
            "def __aura_json_schema_{index}(defs: mut dict[str, {value}]) -> {value}:\n{}",
            indent(&schema)
        ));
        Ok(index)
    }

    /// Generates the object encoder, decoder, and schema for a derived class.
    fn derived_class_helpers(&mut self, name: &str, index: usize) -> Result<()> {
        if !self.classes_done.insert(name.to_string()) {
            return Ok(());
        }
        let Some(class) = self.class(name) else {
            return Ok(());
        };
        let value = format!("{JSON}.Value");
        let error = format!("{JSON}.DecodeError");
        let mut fields = Vec::new();
        for field in &class.fields {
            let shape = self.shape(&field.ty, name, &field.name, 0)?;
            let helper = self.helper(&shape)?;
            fields.push((field, shape.aura_type(), helper));
        }

        let mut encode = format!("mut fields: dict[str, {value}] = {{}}\n");
        for (field, _, helper) in &fields {
            encode.push_str(&format!(
                "fields[\"{0}\"] = __aura_json_encode_{helper}(value.{0})\n",
                field.name
            ));
        }
        encode.push_str(&format!("return {value}.Object(fields)"));

        let mut decode = format!(
            "match own {JSON}.__entries(value):\n\
             \x20   case Result.Err(error):\n\
             \x20       return Result.Err(error)\n\
             \x20   case Result.Ok(entries):\n"
        );
        for (field, ty, _) in &fields {
            decode.push_str(&format!(
                "\x20       mut field_{}: list[{ty}] = []\n",
                field.name
            ));
        }
        decode.push_str("\x20       for key, item in own entries:\n");
        for (position, (field, _, helper)) in fields.iter().enumerate() {
            let keyword = if position == 0 { "if" } else { "elif" };
            decode.push_str(&format!(
                "\x20           {keyword} key == \"{0}\":\n\
                 \x20               match own __aura_json_decode_{helper}(item):\n\
                 \x20                   case Result.Ok(decoded):\n\
                 \x20                       field_{0}.append(decoded)\n\
                 \x20                   case Result.Err(error):\n\
                 \x20                       return Result.Err({JSON}.__at(error, \".{0}\"))\n",
                field.name
            ));
        }
        let unknown = format!(
            "return Result.Err({JSON}.__error({JSON}.DecodeErrorKind.UnknownField(name=key)))"
        );
        if fields.is_empty() {
            decode.push_str(&format!("\x20           {unknown}\n"));
        } else {
            decode.push_str(&format!(
                "\x20           else:\n\x20               {unknown}\n"
            ));
        }
        for (field, ty, _) in &fields {
            decode.push_str(&format!(
                "\x20       if field_{0}.len() == 0:\n",
                field.name
            ));
            match &field.default {
                Some(default) => {
                    let helper = format!("__aura_json_default_{index}_{}", field.name);
                    self.defaults.insert(helper.clone(), default.clone());
                    self.emit(format!("def {helper}() -> {ty}:\n    return None"));
                    decode.push_str(&format!(
                        "\x20           field_{0}.append({helper}())\n",
                        field.name
                    ));
                }
                None => decode.push_str(&format!(
                    "\x20           return Result.Err({JSON}.__error({JSON}.DecodeErrorKind.MissingField(name=\"{}\")))\n",
                    field.name
                )),
            }
        }
        let arguments = fields
            .iter()
            .map(|(field, _, _)| format!("{0}=field_{0}.pop()", field.name))
            .collect::<Vec<_>>()
            .join(", ");
        decode.push_str(&format!("\x20       return Result.Ok({name}({arguments}))"));

        let mut schema = format!("mut properties: dict[str, {value}] = {{}}\n");
        for (field, _, helper) in &fields {
            schema.push_str(&format!(
                "properties[\"{}\"] = __aura_json_schema_{helper}(defs)\n",
                field.name
            ));
        }
        let required = fields
            .iter()
            .filter(|(field, _, _)| field.default.is_none())
            .map(|(field, _, _)| format!("{value}.String(\"{}\")", field.name))
            .collect::<Vec<_>>()
            .join(", ");
        schema.push_str(&format!(
            "required: list[{value}] = [{required}]\n\
             return {value}.Object(\n\
             \x20   {{\n\
             \x20       \"type\": {value}.String(\"object\"),\n\
             \x20       \"properties\": {value}.Object(properties),\n\
             \x20       \"required\": {value}.Array(required),\n\
             \x20       \"additionalProperties\": {value}.Bool(false),\n\
             \x20   }}\n\
             )"
        ));

        self.emit(format!(
            "def __aura_json_encode_class_{index}(value: {name}) -> {value}:\n{}",
            indent(&encode)
        ));
        self.emit(format!(
            "def __aura_json_decode_class_{index}(value: own {value}) -> Result[{name}, {error}]:\n{}",
            indent(&decode)
        ));
        self.emit(format!(
            "def __aura_json_object_{index}(defs: mut dict[str, {value}]) -> {value}:\n{}",
            indent(&schema)
        ));
        Ok(())
    }
}

fn indent(body: &str) -> String {
    body.lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render(ty: &TypeRef) -> String {
    match &ty.kind {
        TypeRefKind::Named { name, args } if args.is_empty() => name.clone(),
        TypeRefKind::Named { name, args } => format!(
            "{name}[{}]",
            args.iter().map(render).collect::<Vec<_>>().join(", ")
        ),
        TypeRefKind::Tuple(items) => format!(
            "({})",
            items.iter().map(render).collect::<Vec<_>>().join(", ")
        ),
        TypeRefKind::Union(members) => members.iter().map(render).collect::<Vec<_>>().join(" | "),
        TypeRefKind::Callable { .. } | TypeRefKind::Function { .. } => "a callable".to_string(),
    }
}
