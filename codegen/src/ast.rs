//! Raw IDL document, deserialized from YAML. Unknown fields are rejected.

use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub idl_version: u32,
    pub abi_version: u32,
    pub errors: Vec<ErrorDef>,
    pub types: IndexMap<String, TypeDef>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorDef {
    pub name: String,
    pub code: i32,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Handle,
    Record,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeDef {
    pub kind: Kind,
    #[serde(default, rename = "abstract")]
    pub is_abstract: bool,
    pub extends: Option<String>,
    pub owner: Option<String>,
    pub dispose: Option<String>,
    pub ctor: Option<Ctor>,
    #[serde(default)]
    pub properties: IndexMap<String, Property>,
    #[serde(default)]
    pub methods: IndexMap<String, Method>,
    #[serde(default)]
    pub shortcuts: IndexMap<String, String>,
    #[serde(default)]
    pub fields: IndexMap<String, Field>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ctor {
    #[serde(default)]
    pub args: Vec<Arg>,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Property {
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Method {
    #[serde(default)]
    pub args: Vec<Arg>,
    #[serde(default = "void")]
    pub returns: String,
    #[serde(default)]
    pub errors: Vec<String>,
}

fn void() -> String {
    "void".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arg {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub optional: bool,
    pub default: Option<serde_yaml::Value>,
}
