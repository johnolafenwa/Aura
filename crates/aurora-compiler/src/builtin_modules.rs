use std::collections::BTreeMap;

use crate::ast::{
    ClassDecl, EnumDecl, EnumPayloadFieldDecl, EnumVariantDecl, FunctionDecl, Param, ReceiverKind,
    TypeRef,
};
use crate::diag::{Diagnostic, Result, Span};
use crate::sema::{
    ClassInfo, EnumInfo, EnumPayloadFieldInfo, EnumVariantInfo, FunctionInfo, FunctionSignature,
    ImportedBinding, ModuleNamespace, Type,
};

fn builtin_span() -> Span {
    Span::new(1, 1)
}

fn type_ref(name: &str, args: Vec<TypeRef>) -> TypeRef {
    TypeRef {
        name: name.to_string(),
        args,
        indirect: false,
        span: builtin_span(),
    }
}

fn lower_type_ref(type_ref: &TypeRef) -> Type {
    if type_ref.name == "None" {
        return Type::Unit;
    }
    let name = if type_ref.name == "str" {
        "String"
    } else {
        &type_ref.name
    };
    Type::Named(
        name.to_string(),
        type_ref.args.iter().map(lower_type_ref).collect(),
    )
}

fn value_param(name: &str, ty: TypeRef) -> Param {
    Param {
        name: name.to_string(),
        passing: ReceiverKind::Value,
        borrow_label: None,
        ty,
        default: None,
        span: builtin_span(),
    }
}

fn function_info(
    module_name: &str,
    name: &str,
    params: Vec<Param>,
    return_type: TypeRef,
) -> FunctionInfo {
    FunctionInfo {
        module_name: module_name.to_string(),
        decl: FunctionDecl {
            public: true,
            name: name.to_string(),
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
            receiver: None,
            params: params.clone(),
            return_passing: ReceiverKind::Value,
            return_borrow_source: None,
            return_type: return_type.clone(),
            body: Vec::new(),
            span: builtin_span(),
        },
        signature: FunctionSignature {
            params: params
                .iter()
                .map(|param| lower_type_ref(&param.ty))
                .collect(),
            return_type: lower_type_ref(&return_type),
            return_passing: ReceiverKind::Value,
            return_borrow_source: None,
        },
        type_param_bounds: BTreeMap::new(),
    }
}

fn class_info(module_name: &str, name: &str) -> ClassInfo {
    ClassInfo {
        module_name: module_name.to_string(),
        decl: ClassDecl {
            public: true,
            copy: false,
            name: name.to_string(),
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            span: builtin_span(),
        },
        type_param_bounds: BTreeMap::new(),
        fields: BTreeMap::new(),
        methods: BTreeMap::new(),
    }
}

fn error_enum_info() -> EnumInfo {
    let variants = vec![
        ("NotFound", Vec::new()),
        ("PermissionDenied", Vec::new()),
        ("AlreadyExists", Vec::new()),
        ("ConnectionRefused", Vec::new()),
        ("ConnectionReset", Vec::new()),
        ("ConnectionAborted", Vec::new()),
        ("NotConnected", Vec::new()),
        ("AddrInUse", Vec::new()),
        ("AddrNotAvailable", Vec::new()),
        ("BrokenPipe", Vec::new()),
        ("TimedOut", Vec::new()),
        ("WouldBlock", Vec::new()),
        ("UnexpectedEof", Vec::new()),
        ("InvalidInput", Vec::new()),
        ("InvalidData", Vec::new()),
        ("Closed", Vec::new()),
        (
            "Other",
            vec![EnumPayloadFieldDecl {
                name: Some("message".to_string()),
                ty: type_ref("String", Vec::new()),
                span: builtin_span(),
            }],
        ),
    ];

    EnumInfo {
        module_name: "io".to_string(),
        decl: EnumDecl {
            public: true,
            name: "Error".to_string(),
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
            variants: variants
                .iter()
                .map(|(name, payloads)| EnumVariantDecl {
                    name: (*name).to_string(),
                    payloads: payloads.clone(),
                    named_payloads: !payloads.is_empty(),
                    span: builtin_span(),
                })
                .collect(),
            span: builtin_span(),
        },
        type_param_bounds: BTreeMap::new(),
        variants: variants
            .into_iter()
            .map(|(name, payloads)| {
                (
                    name.to_string(),
                    EnumVariantInfo {
                        payloads: payloads
                            .iter()
                            .map(|payload| EnumPayloadFieldInfo {
                                name: payload.name.clone(),
                                ty: lower_type_ref(&payload.ty),
                                span: payload.span,
                            })
                            .collect(),
                        named_payloads: !payloads.is_empty(),
                        span: builtin_span(),
                    },
                )
            })
            .collect(),
    }
}

fn io_error_type_ref() -> TypeRef {
    type_ref("io.Error", Vec::new())
}

fn bytes_type_ref() -> TypeRef {
    type_ref("Vec", vec![type_ref("uint8", Vec::new())])
}

fn string_map_type_ref() -> TypeRef {
    type_ref(
        "Map",
        vec![
            type_ref("String", Vec::new()),
            type_ref("String", Vec::new()),
        ],
    )
}

fn result_type_ref(ok: TypeRef) -> TypeRef {
    type_ref("Result", vec![ok, io_error_type_ref()])
}

fn builtin_io_error_type() -> Type {
    Type::Named("io.Error".to_string(), Vec::new())
}

fn io_namespace() -> ModuleNamespace {
    let mut functions = BTreeMap::new();
    for function in [
        function_info(
            "io",
            "write",
            vec![value_param("text", type_ref("String", Vec::new()))],
            type_ref(
                "Result",
                vec![type_ref("None", Vec::new()), io_error_type_ref()],
            ),
        ),
        function_info(
            "io",
            "flush",
            Vec::new(),
            type_ref(
                "Result",
                vec![type_ref("None", Vec::new()), io_error_type_ref()],
            ),
        ),
        function_info(
            "io",
            "read_line",
            Vec::new(),
            type_ref(
                "Result",
                vec![
                    type_ref("Option", vec![type_ref("String", Vec::new())]),
                    io_error_type_ref(),
                ],
            ),
        ),
    ] {
        functions.insert(function.decl.name.clone(), function);
    }

    let error = error_enum_info();
    let mut enums = BTreeMap::new();
    enums.insert(error.decl.name.clone(), error.clone());

    ModuleNamespace {
        name: "io".to_string(),
        path: "io".to_string(),
        source_path: None,
        modules: BTreeMap::new(),
        functions: functions.clone(),
        classes: BTreeMap::new(),
        enums: enums.clone(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: functions,
        all_classes: BTreeMap::new(),
        all_enums: enums,
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    }
}

fn fs_namespace() -> ModuleNamespace {
    let file = class_info("fs", "File");
    let mut classes = BTreeMap::new();
    classes.insert(file.decl.name.clone(), file.clone());

    let result_none = type_ref(
        "Result",
        vec![type_ref("None", Vec::new()), io_error_type_ref()],
    );
    let result_string = type_ref(
        "Result",
        vec![type_ref("String", Vec::new()), io_error_type_ref()],
    );
    let result_file = type_ref(
        "Result",
        vec![type_ref("fs.File", Vec::new()), io_error_type_ref()],
    );
    let result_bytes = type_ref("Result", vec![bytes_type_ref(), io_error_type_ref()]);
    let result_vec_string = type_ref(
        "Result",
        vec![
            type_ref("Vec", vec![type_ref("String", Vec::new())]),
            io_error_type_ref(),
        ],
    );

    let mut functions = BTreeMap::new();
    for function in [
        function_info(
            "fs",
            "exists",
            vec![value_param("path", type_ref("String", Vec::new()))],
            type_ref("bool", Vec::new()),
        ),
        function_info(
            "fs",
            "read_to_string",
            vec![value_param("path", type_ref("String", Vec::new()))],
            result_string.clone(),
        ),
        function_info(
            "fs",
            "read_bytes",
            vec![value_param("path", type_ref("String", Vec::new()))],
            result_bytes.clone(),
        ),
        function_info(
            "fs",
            "write_string",
            vec![
                value_param("path", type_ref("String", Vec::new())),
                value_param("text", type_ref("String", Vec::new())),
            ],
            result_none.clone(),
        ),
        function_info(
            "fs",
            "write_bytes",
            vec![
                value_param("path", type_ref("String", Vec::new())),
                value_param("bytes", bytes_type_ref()),
            ],
            result_none.clone(),
        ),
        function_info(
            "fs",
            "append_string",
            vec![
                value_param("path", type_ref("String", Vec::new())),
                value_param("text", type_ref("String", Vec::new())),
            ],
            result_none.clone(),
        ),
        function_info(
            "fs",
            "append_bytes",
            vec![
                value_param("path", type_ref("String", Vec::new())),
                value_param("bytes", bytes_type_ref()),
            ],
            result_none.clone(),
        ),
        function_info(
            "fs",
            "create_dir",
            vec![value_param("path", type_ref("String", Vec::new()))],
            result_none.clone(),
        ),
        function_info(
            "fs",
            "read_dir",
            vec![value_param("path", type_ref("String", Vec::new()))],
            result_vec_string,
        ),
        function_info(
            "fs",
            "remove_file",
            vec![value_param("path", type_ref("String", Vec::new()))],
            result_none.clone(),
        ),
        function_info(
            "fs",
            "open",
            vec![value_param("path", type_ref("String", Vec::new()))],
            result_file.clone(),
        ),
        function_info(
            "fs",
            "create",
            vec![value_param("path", type_ref("String", Vec::new()))],
            result_file.clone(),
        ),
        function_info(
            "fs",
            "append",
            vec![value_param("path", type_ref("String", Vec::new()))],
            result_file,
        ),
    ] {
        functions.insert(function.decl.name.clone(), function);
    }

    ModuleNamespace {
        name: "fs".to_string(),
        path: "fs".to_string(),
        source_path: None,
        modules: BTreeMap::new(),
        functions: functions.clone(),
        classes: classes.clone(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: functions,
        all_classes: classes,
        all_enums: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    }
}

fn net_namespace() -> ModuleNamespace {
    let stream = class_info("net", "TcpStream");
    let listener = class_info("net", "TcpListener");
    let udp_socket = class_info("net", "UdpSocket");
    let udp_datagram = class_info("net", "UdpDatagram");
    let http_listener = class_info("net", "HttpListener");
    let http_exchange = class_info("net", "HttpExchange");
    let http_response = class_info("net", "HttpResponse");
    let websocket_listener = class_info("net", "WebSocketListener");
    let websocket = class_info("net", "WebSocket");
    let unix_listener = class_info("net", "UnixListener");
    let unix_stream = class_info("net", "UnixStream");
    let tls_listener = class_info("net", "TlsListener");
    let tls_stream = class_info("net", "TlsStream");
    let mut classes = BTreeMap::new();
    classes.insert(stream.decl.name.clone(), stream.clone());
    classes.insert(listener.decl.name.clone(), listener.clone());
    classes.insert(udp_socket.decl.name.clone(), udp_socket.clone());
    classes.insert(udp_datagram.decl.name.clone(), udp_datagram.clone());
    classes.insert(http_listener.decl.name.clone(), http_listener.clone());
    classes.insert(http_exchange.decl.name.clone(), http_exchange.clone());
    classes.insert(http_response.decl.name.clone(), http_response.clone());
    classes.insert(
        websocket_listener.decl.name.clone(),
        websocket_listener.clone(),
    );
    classes.insert(websocket.decl.name.clone(), websocket.clone());
    classes.insert(unix_listener.decl.name.clone(), unix_listener.clone());
    classes.insert(unix_stream.decl.name.clone(), unix_stream.clone());
    classes.insert(tls_listener.decl.name.clone(), tls_listener.clone());
    classes.insert(tls_stream.decl.name.clone(), tls_stream.clone());

    let mut functions = BTreeMap::new();
    for function in [
        function_info(
            "net",
            "connect",
            vec![value_param("address", type_ref("String", Vec::new()))],
            type_ref(
                "Result",
                vec![type_ref("net.TcpStream", Vec::new()), io_error_type_ref()],
            ),
        ),
        function_info(
            "net",
            "connect_timeout",
            vec![
                value_param("address", type_ref("String", Vec::new())),
                value_param("timeout", type_ref("Duration", Vec::new())),
            ],
            type_ref(
                "Result",
                vec![type_ref("net.TcpStream", Vec::new()), io_error_type_ref()],
            ),
        ),
        function_info(
            "net",
            "listen",
            vec![value_param("address", type_ref("String", Vec::new()))],
            type_ref(
                "Result",
                vec![type_ref("net.TcpListener", Vec::new()), io_error_type_ref()],
            ),
        ),
        function_info(
            "net",
            "udp_bind",
            vec![value_param("address", type_ref("String", Vec::new()))],
            result_type_ref(type_ref("net.UdpSocket", Vec::new())),
        ),
        function_info(
            "net",
            "unix_listen",
            vec![value_param("path", type_ref("String", Vec::new()))],
            result_type_ref(type_ref("net.UnixListener", Vec::new())),
        ),
        function_info(
            "net",
            "unix_connect",
            vec![value_param("path", type_ref("String", Vec::new()))],
            result_type_ref(type_ref("net.UnixStream", Vec::new())),
        ),
        function_info(
            "net",
            "unix_connect_timeout",
            vec![
                value_param("path", type_ref("String", Vec::new())),
                value_param("timeout", type_ref("Duration", Vec::new())),
            ],
            result_type_ref(type_ref("net.UnixStream", Vec::new())),
        ),
        function_info(
            "net",
            "tls_listen",
            vec![
                value_param("address", type_ref("String", Vec::new())),
                value_param("cert_pem_path", type_ref("String", Vec::new())),
                value_param("key_pem_path", type_ref("String", Vec::new())),
            ],
            result_type_ref(type_ref("net.TlsListener", Vec::new())),
        ),
        function_info(
            "net",
            "tls_connect",
            vec![
                value_param("address", type_ref("String", Vec::new())),
                value_param("server_name", type_ref("String", Vec::new())),
                value_param("ca_pem_path", type_ref("String", Vec::new())),
            ],
            result_type_ref(type_ref("net.TlsStream", Vec::new())),
        ),
        function_info(
            "net",
            "tls_connect_timeout",
            vec![
                value_param("address", type_ref("String", Vec::new())),
                value_param("server_name", type_ref("String", Vec::new())),
                value_param("ca_pem_path", type_ref("String", Vec::new())),
                value_param("timeout", type_ref("Duration", Vec::new())),
            ],
            result_type_ref(type_ref("net.TlsStream", Vec::new())),
        ),
        function_info(
            "net",
            "http_listen",
            vec![value_param("address", type_ref("String", Vec::new()))],
            result_type_ref(type_ref("net.HttpListener", Vec::new())),
        ),
        function_info(
            "net",
            "http_request_text",
            vec![
                value_param("method", type_ref("String", Vec::new())),
                value_param("url", type_ref("String", Vec::new())),
                value_param("body", type_ref("String", Vec::new())),
                value_param("headers", string_map_type_ref()),
            ],
            result_type_ref(type_ref("net.HttpResponse", Vec::new())),
        ),
        function_info(
            "net",
            "http_request_text_timeout",
            vec![
                value_param("method", type_ref("String", Vec::new())),
                value_param("url", type_ref("String", Vec::new())),
                value_param("body", type_ref("String", Vec::new())),
                value_param("headers", string_map_type_ref()),
                value_param("timeout", type_ref("Duration", Vec::new())),
            ],
            result_type_ref(type_ref("net.HttpResponse", Vec::new())),
        ),
        function_info(
            "net",
            "http_request_bytes",
            vec![
                value_param("method", type_ref("String", Vec::new())),
                value_param("url", type_ref("String", Vec::new())),
                value_param("bytes", bytes_type_ref()),
                value_param("headers", string_map_type_ref()),
            ],
            result_type_ref(type_ref("net.HttpResponse", Vec::new())),
        ),
        function_info(
            "net",
            "http_request_bytes_timeout",
            vec![
                value_param("method", type_ref("String", Vec::new())),
                value_param("url", type_ref("String", Vec::new())),
                value_param("bytes", bytes_type_ref()),
                value_param("headers", string_map_type_ref()),
                value_param("timeout", type_ref("Duration", Vec::new())),
            ],
            result_type_ref(type_ref("net.HttpResponse", Vec::new())),
        ),
        function_info(
            "net",
            "websocket_listen",
            vec![value_param("address", type_ref("String", Vec::new()))],
            result_type_ref(type_ref("net.WebSocketListener", Vec::new())),
        ),
        function_info(
            "net",
            "websocket_connect",
            vec![value_param("url", type_ref("String", Vec::new()))],
            result_type_ref(type_ref("net.WebSocket", Vec::new())),
        ),
        function_info(
            "net",
            "websocket_connect_timeout",
            vec![
                value_param("url", type_ref("String", Vec::new())),
                value_param("timeout", type_ref("Duration", Vec::new())),
            ],
            result_type_ref(type_ref("net.WebSocket", Vec::new())),
        ),
    ] {
        functions.insert(function.decl.name.clone(), function);
    }

    ModuleNamespace {
        name: "net".to_string(),
        path: "net".to_string(),
        source_path: None,
        modules: BTreeMap::new(),
        functions: functions.clone(),
        classes: classes.clone(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: functions,
        all_classes: classes,
        all_enums: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    }
}

fn builtin_root_namespace(name: &str) -> Option<ModuleNamespace> {
    match name {
        "io" => Some(io_namespace()),
        "fs" => Some(fs_namespace()),
        "net" => Some(net_namespace()),
        _ => None,
    }
}

pub(crate) fn builtin_module_namespace(path: &[String]) -> Option<ModuleNamespace> {
    match path {
        [name] => builtin_root_namespace(name),
        _ => None,
    }
}

pub(crate) fn builtin_module_registry() -> BTreeMap<String, ModuleNamespace> {
    ["io", "fs", "net"]
        .into_iter()
        .filter_map(|name| {
            builtin_root_namespace(name).map(|namespace| (name.to_string(), namespace))
        })
        .collect()
}

pub(crate) fn builtin_imported_binding(
    module_path: &[String],
    name: &str,
    span: Span,
) -> Result<ImportedBinding> {
    let namespace = builtin_module_namespace(module_path).ok_or_else(|| {
        Diagnostic::at(
            span,
            format!("cannot resolve builtin module `{}`", module_path.join(".")),
        )
    })?;
    if let Some(function) = namespace.functions.get(name) {
        return Ok(ImportedBinding::Function(function.clone()));
    }
    if let Some(class_info) = namespace.classes.get(name) {
        return Ok(ImportedBinding::Class(class_info.clone()));
    }
    if let Some(enum_info) = namespace.enums.get(name) {
        return Ok(ImportedBinding::Enum(enum_info.clone()));
    }
    if let Some(trait_info) = namespace.traits.get(name) {
        return Ok(ImportedBinding::Trait(trait_info.clone()));
    }
    Err(Diagnostic::at(
        span,
        format!(
            "module `{}` has no export named `{}`",
            module_path.join("."),
            name
        ),
    ))
}

pub(crate) fn io_error_type() -> Type {
    builtin_io_error_type()
}
