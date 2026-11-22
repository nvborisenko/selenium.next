//! Validation of the raw document into a checked model that templates consume.

use crate::ast::{Document, Kind};
use std::collections::HashSet;

#[derive(Debug, thiserror::Error)]
pub enum IdlError {
    #[error("parse error: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Void,
    Bool,
    Int,
    String,
    List(Box<Ty>),
    Named(String),
}

pub fn parse_ty(s: &str) -> Result<Ty, IdlError> {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("list<").and_then(|r| r.strip_suffix('>')) {
        return Ok(Ty::List(Box::new(parse_ty(inner)?)));
    }
    Ok(match s {
        "void" => Ty::Void,
        "bool" => Ty::Bool,
        "int" => Ty::Int,
        "string" => Ty::String,
        n if !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric()) => Ty::Named(n.into()),
        _ => return Err(IdlError::Invalid(format!("malformed type '{s}'"))),
    })
}

pub fn parse(yaml: &str) -> Result<Document, IdlError> {
    let doc: Document = serde_yaml::from_str(yaml)?;
    validate(&doc)?;
    Ok(doc)
}

fn bad<T>(msg: String) -> Result<T, IdlError> {
    Err(IdlError::Invalid(msg))
}

pub fn validate(doc: &Document) -> Result<(), IdlError> {
    let mut codes = HashSet::new();
    let mut names = HashSet::new();
    for e in &doc.errors {
        if !names.insert(e.name.as_str()) || !codes.insert(e.code) {
            return bad(format!("duplicate error '{}' or code {}", e.name, e.code));
        }
    }

    let check_ty = |what: &str, raw: &str| -> Result<(), IdlError> {
        let mut ty = parse_ty(raw)?;
        while let Ty::List(inner) = ty {
            ty = *inner;
        }
        match ty {
            Ty::Named(n) if !doc.types.contains_key(&n) => {
                bad(format!("{what}: unknown type '{n}'"))
            }
            _ => Ok(()),
        }
    };
    let check_errors = |what: &str, list: &[String]| -> Result<(), IdlError> {
        match list.iter().find(|e| !names.contains(e.as_str())) {
            Some(e) => bad(format!("{what}: unknown error '{e}'")),
            None => Ok(()),
        }
    };

    for (name, t) in &doc.types {
        let at = |m: &str| format!("{name}.{m}");
        if t.kind == Kind::Record
            && (t.owner.is_some()
                || t.extends.is_some()
                || t.ctor.is_some()
                || !t.methods.is_empty())
        {
            return bad(format!("{name}: records may only have fields"));
        }
        if t.kind == Kind::Handle && !t.fields.is_empty() {
            return bad(format!("{name}: handles cannot have fields"));
        }
        for (rel, target) in [("extends", &t.extends), ("owner", &t.owner)] {
            if let Some(target) = target {
                match doc.types.get(target) {
                    Some(o) if o.kind == Kind::Handle => {}
                    _ => return bad(format!("{name}: {rel} '{target}' is not a handle type")),
                }
            }
        }
        for (f, field) in &t.fields {
            check_ty(&at(f), &field.ty)?;
        }
        for (p, prop) in &t.properties {
            check_ty(&at(p), &prop.ty)?;
        }
        if let Some(c) = &t.ctor {
            for a in &c.args {
                check_ty(&at("ctor"), &a.ty)?;
            }
            check_errors(&at("ctor"), &c.errors)?;
        }
        for (m, method) in &t.methods {
            check_ty(&at(m), &method.returns)?;
            for a in &method.args {
                check_ty(&at(m), &a.ty)?;
            }
            check_errors(&at(m), &method.errors)?;
        }
        for (alias, path) in &t.shortcuts {
            let (prop, method) = path.split_once('.').ok_or_else(|| {
                IdlError::Invalid(format!("{}: shortcut must be 'property.method'", at(alias)))
            })?;
            let target = match t.properties.get(prop).map(|p| parse_ty(&p.ty)) {
                Some(Ok(Ty::Named(n))) => n,
                _ => return bad(format!("{}: '{prop}' is not a handle property", at(alias))),
            };
            if !doc.types[&target].methods.contains_key(method) {
                return bad(format!("{}: {target} has no method '{method}'", at(alias)));
            }
        }
        for chain in [
            |t: &crate::ast::TypeDef| t.owner.clone(),
            |t: &crate::ast::TypeDef| t.extends.clone(),
        ] {
            let mut cur = chain(t);
            let mut steps = 0;
            while let Some(n) = cur {
                steps += 1;
                if n == *name || steps > doc.types.len() {
                    return bad(format!("{name}: cyclic owner/extends chain"));
                }
                cur = chain(&doc.types[&n]);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = include_str!("../../api/selenium.idl.yaml");

    #[test]
    fn real_idl_is_valid() {
        parse(REAL).unwrap();
    }

    fn err(yaml: &str) -> String {
        parse(yaml).unwrap_err().to_string()
    }

    const HEAD: &str = "idl_version: 1\nabi_version: 1\nerrors: [{name: E, code: 1}]\n";

    #[test]
    fn rejects_unknown_type() {
        let y = format!(
            "{HEAD}types:\n  A:\n    kind: handle\n    properties:\n      p: {{type: Nope}}\n"
        );
        assert!(err(&y).contains("unknown type 'Nope'"));
    }

    #[test]
    fn rejects_unknown_error() {
        let y = format!(
            "{HEAD}types:\n  A:\n    kind: handle\n    methods:\n      m: {{errors: [X]}}\n"
        );
        assert!(err(&y).contains("unknown error 'X'"));
    }

    #[test]
    fn rejects_owner_cycle() {
        let y = format!(
            "{HEAD}types:\n  A: {{kind: handle, owner: B}}\n  B: {{kind: handle, owner: A}}\n"
        );
        assert!(err(&y).contains("cyclic"));
    }

    #[test]
    fn rejects_unknown_field() {
        let y = format!("{HEAD}types:\n  A: {{kind: handle, bidi: x}}\n");
        assert!(err(&y).contains("bidi"));
    }

    #[test]
    fn rejects_async_public_methods() {
        let y = format!(
            "{HEAD}types:\n  A:\n    kind: handle\n    methods:\n      m: {{async: true}}\n"
        );
        assert!(err(&y).contains("async"));
    }

    #[test]
    fn rejects_bad_shortcut() {
        let y = format!("{HEAD}types:\n  A:\n    kind: handle\n    shortcuts: {{go: p.nothing}}\n");
        assert!(err(&y).contains("not a handle property"));
    }
}
