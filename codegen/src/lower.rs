//! Lowers the validated IDL into a flat, C-ABI-level model shared by every template.
//! Per-language type mapping lives here (and only here), so templates stay declarative.

use crate::ast::{Document, Kind, TypeDef};
use crate::model::{parse_ty, IdlError, Ty};
use heck::{ToLowerCamelCase, ToShoutySnakeCase, ToSnakeCase};
use serde::Serialize;

#[derive(Serialize)]
pub struct Abi {
    pub abi_version: u32,
    pub errors: Vec<ErrorCode>,
    pub handles: Vec<Handle>,
    pub types: Vec<TypeInfo>,
    pub records: Vec<Record>,
    pub funcs: Vec<Func>,
}

#[derive(Serialize)]
pub struct ErrorCode {
    pub name: String,
    pub upper: String,
    pub code: i32,
}

#[derive(Serialize)]
pub struct Handle {
    pub name: String,
    pub c_name: String,
    pub release_fn: String,
    pub dispose: Option<Dispose>,
}

/// Public class model for object-oriented bindings.
#[derive(Serialize)]
pub struct TypeInfo {
    pub name: String,
    pub root: String,
    pub extends: Option<String>,
    pub is_root: bool,
    pub is_abstract: bool,
    pub release_fn: String,
    pub dispose_fn: Option<String>,
    pub dispose_method: Option<String>,
}

#[derive(Serialize)]
pub struct Dispose {
    pub c_fn: String,
    pub core_method: String,
}

#[derive(Serialize)]
pub struct Record {
    pub name: String,
    pub c_name: String,
    pub snake: String,
    pub is_input: bool,
    pub fields: Vec<Param>,
}

#[derive(Serialize, Clone)]
pub struct Param {
    pub name: String,
    pub idl_name: String,
    /// bool | int | string | record | handle | list_handle | void
    pub kind: String,
    /// IDL type name for record/handle/list_handle kinds.
    pub ty: Option<String>,
    pub optional: bool,
    pub has_default: bool,
    pub c_type: String,
    pub rust_type: String,
    pub rust_public_type: String,
    pub rust_field_type: String,
    pub cs_type: String,
    pub cs_native: String,
    pub cs_marshal: String,
    pub arg: String,
    pub ts_type: String,
    pub ts_c: String,
    pub ts_init: String,
    pub is_out: bool,
}

#[derive(Serialize)]
pub struct Func {
    pub c_name: String,
    pub idl_name: String,
    /// ctor | method | getter | shortcut
    pub kind: String,
    pub owner: String,
    /// Native (root) handle type receiving the call; none for constructors.
    pub self_type: Option<String>,
    pub self_c_type: Option<String>,
    pub core_method: String,
    /// For shortcuts: the property whose value receives the call.
    pub via: Option<String>,
    pub params: Vec<Param>,
    pub ret: Param,
    pub outs: Vec<Param>,
    /// Full C parameter list: self, params, then out-parameters.
    pub c_params: Vec<Param>,
    pub errors: Vec<String>,
}

fn root<'a>(doc: &'a Document, mut name: &'a str) -> &'a str {
    while let Some(base) = doc.types[name].extends.as_deref() {
        name = base;
    }
    name
}

fn param(doc: &Document, name: &str, raw: &str, optional: bool) -> Result<Param, IdlError> {
    let (kind, ty): (&str, Option<String>) = match parse_ty(raw)? {
        Ty::Void => ("void", None),
        Ty::Bool => ("bool", None),
        Ty::Int => ("int", None),
        Ty::String => ("string", None),
        Ty::Named(n) => match doc.types[&n].kind {
            Kind::Handle => ("handle", Some(root(doc, &n).to_string())),
            Kind::Record => ("record", Some(n)),
        },
        Ty::List(inner) => match *inner {
            Ty::Named(n) if doc.types[&n].kind == Kind::Handle => {
                ("list_handle", Some(root(doc, &n).to_string()))
            }
            _ => {
                return Err(IdlError::Invalid(format!(
                    "'{name}': only list of handles is supported, got '{raw}'"
                )))
            }
        },
    };
    let t = ty.clone().unwrap_or_default();
    let rust_public_type = match kind {
        "bool" => "bool".to_string(),
        "int" => "i32".into(),
        "string" => "&str".into(),
        "handle" => format!("&{t}"),
        "list_handle" => format!("Vec<{t}>"),
        "record" => t.clone(),
        _ => "()".into(),
    };
    let field_type = match kind {
        "string" => "String".to_string(),
        "handle" | "record" => t.clone(),
        _ => rust_public_type.clone(),
    };
    let rust_field_type = if optional {
        format!("Option<{field_type}>")
    } else {
        field_type
    };
    let (c_type, rust_type) = match kind {
        "bool" => ("bool".to_string(), "bool".to_string()),
        "int" => ("int32_t".into(), "i32".into()),
        "string" => ("const char*".into(), "*const c_char".into()),
        "record" => (format!("const Se{t}*"), format!("*const Se{t}")),
        "handle" => (format!("Se{t}*"), format!("*mut core::{t}")),
        "list_handle" => (format!("Se{t}**"), format!("*mut core::{t}")),
        _ => ("void".into(), "()".into()),
    };
    let ts_type = match kind {
        "bool" => "boolean".to_string(),
        "int" => "number".into(),
        "string" => "string".into(),
        "list_handle" => format!("{t}[]"),
        "void" => "void".into(),
        _ => t.clone(),
    };
    let ts_c = match kind {
        "bool" => "bool".to_string(),
        "int" => "int32".into(),
        "string" => "const char *".into(),
        "record" => format!("const Se{t} *"),
        _ => "void *".into(),
    };
    let (cs_type, cs_native, cs_marshal) = match kind {
        "bool" => (
            "bool".to_string(),
            "bool".to_string(),
            "[MarshalAs(UnmanagedType.U1)]".to_string(),
        ),
        "int" => ("int".into(), "int".into(), String::new()),
        "string" => (
            if optional {
                "string?".into()
            } else {
                "string".into()
            },
            "string?".into(),
            "[MarshalAs(UnmanagedType.LPUTF8Str)]".into(),
        ),
        "record" => (
            if optional { format!("{t}?") } else { t.clone() },
            format!("in Native{t}"),
            String::new(),
        ),
        "handle" => (t.clone(), format!("{t}Handle"), String::new()),
        "list_handle" => (
            format!("IReadOnlyList<{t}>"),
            "IntPtr".into(),
            String::new(),
        ),
        _ => ("void".into(), "void".into(), String::new()),
    };
    Ok(Param {
        rust_public_type,
        rust_field_type,
        cs_type,
        cs_native,
        cs_marshal,
        arg: name.to_lower_camel_case(),
        ts_type,
        ts_c,
        ts_init: String::new(),
        is_out: false,
        name: name.to_snake_case(),
        idl_name: name.to_string(),
        kind: kind.into(),
        ty,
        optional,
        has_default: false,
        c_type,
        rust_type,
    })
}

/// C out-parameters carrying a call's result.
fn outs(ret: &Param, doc: &Document) -> Result<Vec<Param>, IdlError> {
    let out = |name: &str,
               arg: &str,
               c: String,
               r: String,
               cs: &str,
               marshal: &str,
               ts: &str,
               init: &str| Param {
        name: name.into(),
        c_type: c,
        rust_type: r,
        cs_native: cs.into(),
        cs_marshal: marshal.into(),
        arg: arg.into(),
        ts_c: ts.into(),
        ts_init: init.into(),
        is_out: true,
        ..ret.clone()
    };
    let t = ret.ty.clone().unwrap_or_default();
    let primitive = |kind: &str, name: &str, arg: &str, ty: &str, idl_name: &str| {
        let mut p = match kind {
            "bool" => out(
                name,
                arg,
                "bool*".into(),
                "*mut bool".into(),
                "out bool",
                "[MarshalAs(UnmanagedType.U1)]",
                "_Out_ bool *",
                "false",
            ),
            "int" => out(
                name,
                arg,
                "int32_t*".into(),
                "*mut i32".into(),
                "out int",
                "",
                "_Out_ int32 *",
                "0",
            ),
            "string" => out(
                name,
                arg,
                "char**".into(),
                "*mut *mut c_char".into(),
                "out IntPtr",
                "",
                "_Out_ void **",
                "null",
            ),
            _ => unreachable!("unsupported record field kind"),
        };
        p.ty = Some(ty.to_string());
        p.idl_name = idl_name.to_string();
        p.kind = kind.to_string();
        p.ts_type = match kind {
            "bool" => "boolean",
            "int" => "number",
            "string" => "string",
            _ => unreachable!("unsupported record field kind"),
        }
        .to_string();
        p
    };
    Ok(match ret.kind.as_str() {
        "bool" => vec![out(
            "out",
            "result",
            "bool*".into(),
            "*mut bool".into(),
            "out bool",
            "[MarshalAs(UnmanagedType.U1)]",
            "_Out_ bool *",
            "false",
        )],
        "int" => vec![out(
            "out",
            "result",
            "int32_t*".into(),
            "*mut i32".into(),
            "out int",
            "",
            "_Out_ int32 *",
            "0",
        )],
        "string" => vec![out(
            "out",
            "result",
            "char**".into(),
            "*mut *mut c_char".into(),
            "out IntPtr",
            "",
            "_Out_ void **",
            "null",
        )],
        "handle" => vec![out(
            "out",
            "result",
            format!("Se{t}**"),
            format!("*mut *mut core::{t}"),
            "out IntPtr",
            "",
            "_Out_ void **",
            "null",
        )],
        "record" => doc.types[&t]
            .fields
            .iter()
            .map(|(name, field)| {
                let field = param(doc, name, &field.ty, false)?;
                Ok(primitive(
                    &field.kind,
                    &field.name,
                    &field.arg,
                    field.ty.as_deref().unwrap_or(""),
                    &field.idl_name,
                ))
            })
            .collect::<Result<Vec<_>, IdlError>>()?,
        "list_handle" => vec![
            out(
                "out_items",
                "items",
                format!("Se{t}***"),
                format!("*mut *mut *mut core::{t}"),
                "out IntPtr",
                "",
                "_Out_ void **",
                "null",
            ),
            out(
                "out_len",
                "length",
                "size_t*".into(),
                "*mut usize".into(),
                "out UIntPtr",
                "",
                "_Out_ size_t *",
                "0",
            ),
        ],
        _ => vec![],
    })
}

pub fn lower(doc: &Document) -> Result<Abi, IdlError> {
    validate_rust_target(doc)?;
    let mut handles = vec![];
    let mut types = vec![];
    let mut records = vec![];
    let mut funcs = vec![];

    for (name, t) in &doc.types {
        match t.kind {
            Kind::Record => {
                let fields = t
                    .fields
                    .iter()
                    .map(|(f, def)| {
                        let mut field = param(doc, f, &def.ty, def.optional)?;
                        field.has_default = def.default.is_some();
                        Ok::<Param, IdlError>(field)
                    })
                    .collect::<Result<_, _>>()?;
                records.push(Record {
                    name: name.clone(),
                    c_name: format!("Se{name}"),
                    snake: name.to_snake_case(),
                    is_input: false,
                    fields,
                });
            }
            Kind::Handle => {
                let native = root(doc, name).to_string();
                let root_def = &doc.types[&native];
                types.push(TypeInfo {
                    name: name.clone(),
                    root: native.clone(),
                    extends: t.extends.clone(),
                    is_root: native == *name,
                    is_abstract: t.is_abstract,
                    release_fn: format!("se_{}_release", native.to_snake_case()),
                    dispose_fn: root_def
                        .dispose
                        .as_ref()
                        .map(|d| format!("se_{}_{}", native.to_snake_case(), d.to_snake_case())),
                    dispose_method: root_def.dispose.as_ref().map(|d| d.to_snake_case()),
                });
                if native == *name {
                    let snake = name.to_snake_case();
                    handles.push(Handle {
                        name: name.clone(),
                        c_name: format!("Se{name}"),
                        release_fn: format!("se_{snake}_release"),
                        dispose: t.dispose.as_ref().map(|d| Dispose {
                            c_fn: format!("se_{snake}_{}", d.to_snake_case()),
                            core_method: d.to_snake_case(),
                        }),
                    });
                }
                lower_handle(doc, name, t, &native, &mut funcs)?;
            }
        }
    }

    for record in &mut records {
        record.is_input = funcs.iter().any(|func| {
            func.params
                .iter()
                .any(|param| param.kind == "record" && param.ty.as_deref() == Some(&record.name))
        });
    }

    Ok(Abi {
        abi_version: doc.abi_version,
        errors: doc
            .errors
            .iter()
            .map(|e| ErrorCode {
                name: e.name.clone(),
                upper: e.name.to_shouty_snake_case(),
                code: e.code,
            })
            .collect(),
        handles,
        types,
        records,
        funcs,
    })
}

fn validate_rust_target(doc: &Document) -> Result<(), IdlError> {
    let unsupported = |message: String| IdlError::Invalid(format!("Rust binding: {message}"));
    for (name, def) in &doc.types {
        if let Some(ctor) = &def.ctor {
            let supported = def
                .extends
                .as_ref()
                .is_some_and(|parent| doc.types[parent].extends.is_none())
                && ctor.args.len() == 1
                && ctor.args[0].optional
                && matches!(
                    parse_ty(&ctor.args[0].ty)?,
                    Ty::Named(ref ty) if doc.types[ty].kind == Kind::Record
                );
            if !supported {
                return Err(unsupported(format!(
                    "{name}: constructors require one optional options record on a derived handle"
                )));
            }
        }
        for (property_name, property) in &def.properties {
            if let Ty::Named(ty) = parse_ty(&property.ty)? {
                if doc.types[&ty].kind == Kind::Record {
                    return Err(unsupported(format!(
                        "{name}.{property_name}: record properties are not supported"
                    )));
                }
            }
        }
        for (field_name, field) in &def.fields {
            let ty = parse_ty(&field.ty)?;
            if !matches!(ty, Ty::Bool | Ty::Int | Ty::String) {
                return Err(unsupported(format!(
                    "{name}.{field_name}: unsupported record field type"
                )));
            }
            if let Some(default) = &field.default {
                let matches_derived_default = !field.optional
                    && match (&ty, default) {
                        (Ty::Bool, serde_yaml::Value::Bool(false)) => true,
                        (Ty::Int, serde_yaml::Value::Number(n)) => n.as_i64() == Some(0),
                        (Ty::String, serde_yaml::Value::String(s)) => s.is_empty(),
                        _ => false,
                    };
                if !matches_derived_default {
                    return Err(unsupported(format!(
                        "{name}.{field_name}: unsupported field default"
                    )));
                }
            }
        }
        for (method_name, method) in &def.methods {
            if method.args.iter().any(|arg| arg.optional) {
                return Err(unsupported(format!(
                    "{name}.{method_name}: optional method arguments are not supported"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_target_rejects_unsupported_field_defaults() {
        let source =
            include_str!("../../api/selenium.idl.yaml").replace("default: false", "default: true");
        let document = crate::model::parse(&source).unwrap();
        let error = lower(&document).err().unwrap().to_string();
        assert!(error.contains("ChromeOptions.headless: unsupported field default"));
    }

    #[test]
    fn record_returns_expand_to_typed_output_fields() {
        let document = crate::model::parse(include_str!("../../api/selenium.idl.yaml")).unwrap();
        let abi = lower(&document).unwrap();
        let get_status = abi
            .funcs
            .iter()
            .find(|f| f.idl_name == "getStatus")
            .unwrap();

        assert_eq!(
            get_status
                .outs
                .iter()
                .map(|out| (out.name.as_str(), out.kind.as_str(), out.ts_type.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("ready", "bool", "boolean"),
                ("message", "string", "string")
            ]
        );
        assert!(
            abi.records
                .iter()
                .find(|r| r.name == "ChromeOptions")
                .unwrap()
                .is_input
        );
        assert!(
            !abi.records
                .iter()
                .find(|r| r.name == "BrowserStatus")
                .unwrap()
                .is_input
        );
    }
}

fn lower_handle(
    doc: &Document,
    name: &str,
    t: &TypeDef,
    native: &str,
    funcs: &mut Vec<Func>,
) -> Result<(), IdlError> {
    let native_snake = native.to_snake_case();
    let args = |list: &[crate::ast::Arg]| -> Result<Vec<Param>, IdlError> {
        list.iter()
            .map(|a| param(doc, &a.name, &a.ty, a.optional))
            .collect()
    };
    let mut push = |c_name: String,
                    idl: &str,
                    kind: &str,
                    self_type: bool,
                    core_method: String,
                    via: Option<String>,
                    params: Vec<Param>,
                    ret: Param,
                    errors: Vec<String>| {
        let this = Param {
            name: "self".into(),
            idl_name: "self".into(),
            kind: "handle".into(),
            ty: Some(native.to_string()),
            optional: false,
            has_default: false,
            c_type: format!("Se{native}*"),
            rust_type: format!("*mut core::{native}"),
            rust_public_type: format!("&{native}"),
            rust_field_type: native.to_string(),
            cs_type: native.to_string(),
            cs_native: format!("{native}Handle"),
            cs_marshal: String::new(),
            arg: "self".into(),
            ts_type: native.to_string(),
            ts_c: "void *".into(),
            ts_init: String::new(),
            is_out: false,
        };
        let outputs = outs(&ret, doc).expect("return type validated");
        let c_params = self_type
            .then_some(this)
            .into_iter()
            .chain(params.iter().cloned())
            .chain(outputs.iter().cloned())
            .collect();
        funcs.push(Func {
            c_params,
            c_name,
            idl_name: idl.into(),
            kind: kind.into(),
            owner: name.into(),
            self_type: self_type.then(|| native.to_string()),
            self_c_type: self_type.then(|| format!("Se{native}")),
            core_method,
            via,
            outs: outputs,
            params,
            ret,
            errors,
        });
    };

    if let Some(c) = &t.ctor {
        let ret = param(doc, "result", native, false)?;
        push(
            format!("se_{}_new", name.to_snake_case()),
            "new",
            "ctor",
            false,
            "new".into(),
            None,
            args(&c.args)?,
            ret,
            c.errors.clone(),
        );
    }
    for (p, prop) in &t.properties {
        let ret = param(doc, p, &prop.ty, false)?;
        push(
            format!("se_{native_snake}_get_{}", p.to_snake_case()),
            p,
            "getter",
            true,
            p.to_snake_case(),
            None,
            vec![],
            ret,
            vec![],
        );
    }
    for (m, method) in &t.methods {
        let ret = param(doc, m, &method.returns, false)?;
        push(
            format!("se_{native_snake}_{}", m.to_snake_case()),
            m,
            "method",
            true,
            m.to_snake_case(),
            None,
            args(&method.args)?,
            ret,
            method.errors.clone(),
        );
    }
    for (alias, path) in &t.shortcuts {
        let (prop, method) = path.split_once('.').expect("validated");
        let Ty::Named(target) = parse_ty(&t.properties[prop].ty)? else {
            unreachable!("validated")
        };
        let target_method = &doc.types[&target].methods[method];
        let ret = param(doc, alias, &target_method.returns, false)?;
        push(
            format!("se_{native_snake}_{}", alias.to_snake_case()),
            alias,
            "shortcut",
            true,
            method.to_snake_case(),
            Some(prop.to_snake_case()),
            args(&target_method.args)?,
            ret,
            target_method.errors.clone(),
        );
    }
    Ok(())
}
