use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fmt::Write as _,
    fs,
    path::PathBuf,
};

use serde_json::Value;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let catalog_path = manifest_dir
        .join("../rustling-ai-engine/src/operation_catalog.json")
        .canonicalize()?;
    println!("cargo:rerun-if-changed={}", catalog_path.display());

    let catalog_text = fs::read_to_string(&catalog_path)?;
    let catalog: BTreeMap<String, Value> = serde_json::from_str(&catalog_text)?;
    let generated = generate_bindings(&catalog)?;
    let output_path = PathBuf::from(env::var("OUT_DIR")?).join("operation_bindings.rs");
    fs::write(output_path, generated)?;
    Ok(())
}

fn generate_bindings(catalog: &BTreeMap<String, Value>) -> Result<String, Box<dyn Error>> {
    let mut ids = BTreeSet::new();
    let mut output = String::from(
        "// Generated from rustling-ai-engine/src/operation_catalog.json.\n\
         // Do not edit by hand.\n\
         pub const GENERATED_OPERATIONS: &[OperationBinding] = &[\n",
    );
    for (path, schema) in catalog {
        let id = operation_id(path).ok_or_else(|| format!("unsupported catalog path: {path}"))?;
        if !ids.insert(id.clone()) {
            return Err(format!("duplicate generated operation id: {id}").into());
        }
        let title = schema
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("catalog operation {path} has no title"))?;
        let schema_json = serde_json::to_string(schema)?;
        writeln!(
            output,
            "    OperationBinding {{ id: {id:?}, path: {path:?}, title: {title:?}, schema_json: {schema_json:?} }},"
        )?;
    }
    output.push_str("];\n");
    Ok(output)
}

fn operation_id(path: &str) -> Option<String> {
    path.strip_prefix("/api/v1/")
        .filter(|relative| !relative.is_empty())
        .map(|relative| relative.replace('/', "-"))
}

#[cfg(test)]
mod tests {
    use super::operation_id;

    #[test]
    fn derives_stable_operation_id_from_api_path() {
        assert_eq!(
            operation_id("/api/v1/general/rotate-pdf").as_deref(),
            Some("general-rotate-pdf")
        );
        assert_eq!(operation_id("/other/path"), None);
    }
}
