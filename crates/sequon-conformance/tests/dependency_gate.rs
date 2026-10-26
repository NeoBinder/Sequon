//! Dependency and workspace policy gate.
//!
//! This test parses the repository manifests and `Cargo.lock` directly, without
//! adding a TOML dependency, and asserts the rules in `AGENTS.md`,
//! `docs/design/snapgene-dna.md`, plan section 7.8 and `deny.toml`:
//!
//! - no third-party domain parser appears in the lockfile or a manifest;
//! - the library crates keep exactly the documented runtime dependencies;
//! - the general-purpose codecs stay optional and behind the snapgene feature;
//! - the conformance harness and the fuzz crate stay dev-only
//!   (\`publish = false\`) and outside the runtime graph;
//! - the fuzz crate is its own workspace, so stable \`cargo test\` never builds it.
//!
//! A failure here is a policy violation, not a style warning.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const BANNED_PARSERS: &[&str] = &["gb-io", "sgffp", "biopython", "teselagen", "bio-parsers"];

/// Direct dependencies of one manifest section.
struct ManifestDependencies {
    normal: BTreeSet<String>,
    optional: BTreeSet<String>,
    dev: BTreeSet<String>,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives at <workspace>/crates/sequon-conformance")
        .to_path_buf()
}

/// Extracts dependency sections from a Cargo manifest.
///
/// The workspace manifests use one dependency per line, so a line-oriented
/// reader is sufficient and keeps this test dependency-free. A dependency
/// entry looks like `name = { ... }` or `name = "1"`; the optional flag
/// is detected when `optional = true` appears on the same line.
fn parse_manifest(path: &Path) -> ManifestDependencies {
    let text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let mut dependencies = ManifestDependencies {
        normal: BTreeSet::new(),
        optional: BTreeSet::new(),
        dev: BTreeSet::new(),
    };
    let mut section = "";
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed;
            continue;
        }
        let target = match section {
            "[dependencies]" => &mut dependencies.normal,
            "[dev-dependencies]" => &mut dependencies.dev,
            _ => continue,
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let name = key.trim();
        if name.is_empty() || name.starts_with('#') {
            continue;
        }
        target.insert(name.to_owned());
        if value.contains("optional = true") || value.contains("optional=true") {
            dependencies.optional.insert(name.to_owned());
        }
    }
    dependencies
}

/// Minimal `Cargo.lock` package reader: package name -> direct dependencies.
fn parse_lockfile(path: &Path) -> Vec<(String, String, Vec<String>)> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let mut packages: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_version = String::new();
    let mut current_dependencies: Vec<String> = Vec::new();
    let mut in_dependencies = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            if let Some(name) = current_name.take() {
                packages.push((
                    name,
                    std::mem::take(&mut current_version),
                    std::mem::take(&mut current_dependencies),
                ));
            }
            in_dependencies = false;
            continue;
        }
        if in_dependencies {
            if trimmed == "]" {
                in_dependencies = false;
                continue;
            }
            let entry = trimmed.trim_matches(|character| character == '"' || character == ',');
            if !entry.is_empty() {
                current_dependencies.push(entry.split(' ').next().unwrap_or(entry).to_owned());
            }
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("name = ") {
            current_name = Some(value.trim_matches('"').to_owned());
        } else if let Some(value) = trimmed.strip_prefix("version = ") {
            current_version = value.trim_matches('"').to_owned();
        } else if trimmed == "dependencies = [" {
            in_dependencies = true;
        }
    }
    if let Some(name) = current_name {
        packages.push((name, current_version, current_dependencies));
    }
    packages
}

fn assert_no_banned_parser(name: &str, origin: &Path) {
    assert!(
        !BANNED_PARSERS.contains(&name),
        "{}: banned third-party domain parser '{name}'",
        origin.display()
    );
}

#[test]
fn lockfile_and_manifests_contain_no_third_party_domain_parser() {
    let root = workspace_root();
    let lock_path = root.join("Cargo.lock");
    let packages = parse_lockfile(&lock_path);
    assert!(!packages.is_empty(), "Cargo.lock parsed no packages");
    for (name, _, _) in &packages {
        assert_no_banned_parser(name, &lock_path);
    }

    // Also scan every manifest under the repository (workspace crates and the
    // fuzz workspace), because a banned crate could be added without a rebuild.
    let mut manifests = vec![
        root.join("Cargo.toml"),
        root.join("crates/sequon-conformance/Cargo.toml"),
    ];
    for directory in [
        "crates/sequon-model",
        "crates/sequon-algo",
        "crates/sequon-io",
        "fuzz",
    ] {
        manifests.push(root.join(directory).join("Cargo.toml"));
    }
    for manifest in &manifests {
        let text = fs::read_to_string(manifest)
            .unwrap_or_else(|error| panic!("read {}: {error}", manifest.display()));
        for banned in BANNED_PARSERS {
            assert!(
                !text.contains(banned),
                "{} mentions the banned parser '{banned}'",
                manifest.display()
            );
        }
    }
}

#[test]
fn library_crates_keep_their_documented_runtime_dependencies() {
    let root = workspace_root();
    let model = parse_manifest(&root.join("crates/sequon-model/Cargo.toml"));
    let algo = parse_manifest(&root.join("crates/sequon-algo/Cargo.toml"));
    let io = parse_manifest(&root.join("crates/sequon-io/Cargo.toml"));

    assert_eq!(
        names(&model.normal),
        vec!["serde"],
        "sequon-model depends on serde only"
    );
    assert!(
        model.dev.is_empty(),
        "sequon-model must not gain dev-dependencies"
    );

    assert_eq!(
        names(&algo.normal),
        vec!["sequon-model", "sha2"],
        "sequon-algo depends on sequon-model and sha2"
    );
    assert!(
        !algo
            .normal
            .iter()
            .any(|name| name == "serde_json" || name == "lzma-rs" || name == "quick-xml"),
        "sequon-algo must not depend on JSON parsing or a codec"
    );

    assert_eq!(
        names(&io.normal),
        vec![
            "lzma-rs",
            "quick-xml",
            "sequon-algo",
            "sequon-model",
            "serde",
            "serde_json"
        ],
        "sequon-io runtime dependencies changed"
    );
    for codec in ["lzma-rs", "quick-xml"] {
        assert!(
            io.optional.contains(codec),
            "sequon-io codec {codec} must be optional"
        );
    }
    let io_manifest =
        fs::read_to_string(root.join("crates/sequon-io/Cargo.toml")).expect("io manifest");
    assert!(
        io_manifest.contains("snapgene = [\"dep:lzma-rs\", \"dep:quick-xml\"]"),
        "the snapgene feature must be the only thing that enables the codecs"
    );

    // Neither library crate may depend on the dev-only harness.
    for (label, dependencies) in [
        ("sequon-model", &model.normal),
        ("sequon-algo", &algo.normal),
        ("sequon-io", &io.normal),
    ] {
        assert!(
            !dependencies.contains("sequon-conformance"),
            "{label} must not depend on the conformance harness"
        );
    }
}

#[test]
fn dev_only_crates_stay_published_false_and_outside_the_runtime_graph() {
    let root = workspace_root();
    let conformance = fs::read_to_string(root.join("crates/sequon-conformance/Cargo.toml"))
        .expect("conformance manifest");
    assert!(conformance.contains("publish = false"));

    // The C9 compare binary imports, validates and digest-checks records, so the
    // workspace crates are runtime dependencies of this dev-only harness. The
    // dependency direction is still one-way: no library crate may depend on it.
    let harness = parse_manifest(&root.join("crates/sequon-conformance/Cargo.toml"));
    assert_eq!(
        names(&harness.normal),
        vec![
            "lzma-rs",
            "sequon-algo",
            "sequon-io",
            "sequon-model",
            "serde",
            "serde_json"
        ],
        "conformance runtime dependencies changed"
    );
    assert_eq!(
        names(&harness.dev),
        vec!["jsonschema", "sha2"],
        "conformance dev-dependencies changed"
    );
    assert!(
        !harness.normal.contains("jsonschema"),
        "jsonschema is a schema-test dev-dependency, not a runtime dependency"
    );
    assert!(
        !harness.normal.contains("sha2"),
        "sha2 is a fixture-test dev-dependency, not a runtime dependency"
    );

    for library in [
        "crates/sequon-model/Cargo.toml",
        "crates/sequon-algo/Cargo.toml",
        "crates/sequon-io/Cargo.toml",
    ] {
        let text = fs::read_to_string(root.join(library)).expect("library manifest");
        assert!(!text.contains("sequon-conformance"));
        // Covers a target-specific or build-dependency section too: a library
        // crate must never name the schema engine anywhere in its manifest.
        assert!(
            !text.contains("jsonschema"),
            "{library} must not depend on the dev-only jsonschema crate"
        );
    }

    // Lockfile graph: only the dev-only harness may depend on jsonschema. This
    // catches an indirect or target-specific runtime route that the direct
    // manifest assertions above would not see.
    let packages = parse_lockfile(&root.join("Cargo.lock"));
    for (name, _, dependencies) in &packages {
        if dependencies
            .iter()
            .any(|dependency| dependency == "jsonschema")
        {
            assert_eq!(
                name, "sequon-conformance",
                "only the dev-only conformance harness may depend on jsonschema"
            );
        }
    }

    let root_manifest = fs::read_to_string(root.join("Cargo.toml")).expect("workspace manifest");
    assert!(
        !root_manifest.contains("\"fuzz\""),
        "fuzz must not be a workspace member"
    );

    let fuzz = fs::read_to_string(root.join("fuzz/Cargo.toml")).expect("fuzz manifest");
    assert!(
        fuzz.contains("[workspace]"),
        "fuzz must be its own workspace"
    );
    assert!(fuzz.contains("publish = false"));
    assert!(fuzz.contains("libfuzzer-sys"));
    for target in ["genbank", "dna", "location"] {
        assert!(
            root.join(format!("fuzz/fuzz_targets/{target}.rs"))
                .is_file(),
            "missing fuzz target {target}"
        );
    }
}

#[test]
fn lockfile_keeps_the_pinned_general_purpose_codecs() {
    let root = workspace_root();
    let packages = parse_lockfile(&root.join("Cargo.lock"));
    for (name, expected) in [("lzma-rs", "0.3.0"), ("quick-xml", "0.42.0")] {
        let package = packages
            .iter()
            .find(|(package, _, _)| package == name)
            .unwrap_or_else(|| panic!("Cargo.lock is missing the documented codec {name}"));
        assert_eq!(
            package.1, expected,
            "{name} must stay pinned exactly as documented"
        );
    }
}

fn names(dependencies: &BTreeSet<String>) -> Vec<&str> {
    dependencies.iter().map(String::as_str).collect()
}
