// Shared enforcement; each crate keeps its responsibility and allowlist beside
// its manifest. Parse TOML so aliases, dependency tables, and target sections
// cannot change which package the guard checks.

use toml_edit::{DocumentMut, Item, TableLike};

#[cfg(not(test))]
pub(crate) fn enforce(name: &str, allowed: &[&str], reason: &str, file: &str) {
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    let manifest = std::fs::read_to_string("Cargo.toml").expect("read own Cargo.toml");
    let workspace = std::fs::read_to_string("../../Cargo.toml").expect("read workspace Cargo.toml");
    if let Err(error) = check(&manifest, &workspace, allowed) {
        panic!("{name}: {error}\n\n{reason}\n\nChange the allowlist deliberately in {file} if the architecture changed.");
    }
}

fn check(manifest: &str, workspace: &str, allowed: &[&str]) -> Result<(), String> {
    let manifest = manifest.parse::<DocumentMut>().map_err(|error| error.to_string())?;
    let workspace = workspace.parse::<DocumentMut>().map_err(|error| error.to_string())?;
    let inherited = workspace.get("workspace").and_then(|item| item.get("dependencies"));
    check_table(manifest.as_table(), inherited, allowed)?;
    if let Some(targets) = manifest.get("target").and_then(Item::as_table_like) {
        for (_, target) in targets.iter() {
            check_table(target.as_table_like().ok_or("invalid target table")?, inherited, allowed)?;
        }
    }
    Ok(())
}

fn check_table(table: &dyn TableLike, inherited: Option<&Item>, allowed: &[&str]) -> Result<(), String> {
    for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(dependencies) = table.get(kind) else { continue; };
        let dependencies = dependencies.as_table_like().ok_or("invalid dependency table")?;
        // This parser runs in the build script, never in sim or gameplay.
        // Build dependencies have a separate, deliberately minimal allowlist.
        let permitted = if kind == "build-dependencies" { &["toml_edit"][..] } else { allowed };
        for (alias, dependency) in dependencies.iter() {
            let definition = if dependency.get("workspace").and_then(Item::as_bool) == Some(true) {
                inherited.and_then(|table| table.get(alias))
                    .ok_or_else(|| format!("missing workspace dependency {alias}"))?
            } else { dependency };
            let package = definition.get("package").and_then(Item::as_str).unwrap_or(alias);
            if !permitted.contains(&package) {
                return Err(format!("{kind}: {alias} names forbidden package {package}; permitted: {}",
                    permitted.join(", ")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn forbidden_packages_are_rejected_in_every_dependency_spelling() {
        for section in ["dependencies", "dev-dependencies", "build-dependencies",
            "target.'cfg(target_os = \"macos\")'.dependencies",
            "target.'cfg(target_os = \"macos\")'.dev-dependencies",
            "target.'cfg(target_os = \"macos\")'.build-dependencies"] {
            for entry in [
                "wgpu = '30'",
                "glam = { package = 'wgpu', version = '30' }",
                "glam.workspace = true",
            ] {
                let manifest = format!("[{section}]\n{entry}");
                let workspace = "[workspace.dependencies]\nglam = { package = 'wgpu', version = '30' }";
                assert!(check(&manifest, workspace, &["glam"]).unwrap_err().contains("wgpu"),
                    "{manifest}");
            }
            let table = format!("[{section}.glam]\npackage = 'wgpu'\nversion = '30'");
            assert!(check(&table, "", &["glam"]).unwrap_err().contains("wgpu"));
        }
    }

    #[test]
    fn allowed_packages_can_be_renamed_inherited_or_declared_as_tables() {
        let workspace = "[workspace.dependencies]\nmath = { package = 'glam', version = '0.33' }";
        for manifest in [
            "[dependencies]\nmath = { package = 'glam', version = '0.33' }",
            "[dependencies.math]\npackage = 'glam'\nversion = '0.33'",
            "[dependencies]\nmath.workspace = true",
            "[build-dependencies]\ntoml_edit = '0.25'",
            "# wgpu is forbidden\n[package]\nname = 'wgpu'",
        ] {
            assert_eq!(check(manifest, workspace, &["glam"]), Ok(()), "{manifest}");
        }
        assert!(check("[dependencies]\nmath.workspace = true", "", &["math"]).is_err());
        assert!(check("[dependencies]\ntoml_edit = '0.25'", "", &["glam"]).is_err(),
            "build plumbing must not widen runtime authority");
    }
}
