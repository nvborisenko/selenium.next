//! Renders templates and writes (or verifies) the generated files.

use crate::lower::Abi;
use heck::{ToSnakeCase, ToUpperCamelCase};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tera::{Context, Tera, Value};

struct Target {
    template: &'static str,
    source: &'static str,
    output: &'static str,
    rustfmt: bool,
}

const TARGETS: &[Target] = &[
    Target {
        template: "ffi.rs",
        source: include_str!("../../core/selenium-ffi/templates/ffi.rs.tera"),
        output: "core/selenium-ffi/src/generated.rs",
        rustfmt: true,
    },
    Target {
        template: "selenium.h",
        source: include_str!("../../core/selenium-ffi/templates/selenium.h.tera"),
        output: "core/selenium-ffi/include/selenium.h",
        rustfmt: false,
    },
    Target {
        template: "Native.g.cs",
        source: include_str!("../../api/dotnet/templates/Native.g.cs.tera"),
        output: "api/dotnet/src/Selenium.Next/Generated/Native.g.cs",
        rustfmt: false,
    },
    Target {
        template: "Api.g.cs",
        source: include_str!("../../api/dotnet/templates/Api.g.cs.tera"),
        output: "api/dotnet/src/Selenium.Next/Generated/Api.g.cs",
        rustfmt: false,
    },
    Target {
        template: "native.g.ts",
        source: include_str!("../../api/ts/templates/native.g.ts.tera"),
        output: "api/ts/src/generated/native.g.ts",
        rustfmt: false,
    },
    Target {
        template: "api.g.ts",
        source: include_str!("../../api/ts/templates/api.g.ts.tera"),
        output: "api/ts/src/generated/api.g.ts",
        rustfmt: false,
    },
    Target {
        template: "api.rs",
        source: include_str!("../../api/rust/templates/api.rs.tera"),
        output: "api/rust/src/generated.rs",
        rustfmt: true,
    },
];

fn snake_filter(v: &Value, _: &HashMap<String, Value>) -> tera::Result<Value> {
    Ok(Value::String(
        v.as_str().unwrap_or_default().to_snake_case(),
    ))
}

fn pascal_filter(v: &Value, _: &HashMap<String, Value>) -> tera::Result<Value> {
    Ok(Value::String(
        v.as_str().unwrap_or_default().to_upper_camel_case(),
    ))
}

fn rustfmt(src: String) -> String {
    let child = Command::new("rustfmt")
        .args(["--edition", "2021"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return src };
    use std::io::{Read, Write};
    let mut stdin = child.stdin.take().expect("piped");
    let input = src.clone();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let mut out = String::new();
    child
        .stdout
        .take()
        .expect("piped")
        .read_to_string(&mut out)
        .ok();
    let _ = writer.join();
    match child.wait() {
        Ok(s) if s.success() => out,
        _ => src,
    }
}

pub fn render_all(abi: &Abi) -> Result<Vec<(PathBuf, String)>, String> {
    let mut tera = Tera::default();
    tera.autoescape_on(vec![]);
    tera.register_filter("snake", snake_filter);
    tera.register_filter("pascal", pascal_filter);
    for t in TARGETS {
        tera.add_raw_template(t.template, t.source)
            .map_err(|e| format!("{e:?}"))?;
    }
    let ctx = Context::from_serialize(abi).map_err(|e| e.to_string())?;
    TARGETS
        .iter()
        .map(|t| {
            let mut text = tera
                .render(t.template, &ctx)
                .map_err(|e| format!("{}: {e:?}", t.template))?;
            if t.rustfmt {
                text = rustfmt(text);
            }
            if !text.ends_with('\n') {
                text.push('\n');
            }
            Ok((PathBuf::from(t.output), text))
        })
        .collect()
}
