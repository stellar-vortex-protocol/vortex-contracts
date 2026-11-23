//! Contract-spec (ABI) compatibility checker.
//!
//! Extracts the `contractspecv0` entries embedded in built wasm artifacts,
//! compares them against the committed baselines in `specs/<crate>.json`, and
//! fails on breaking changes. Additive changes are reported but allowed.
//!
//! Breaking changes must be explicitly acknowledged via an allowlist entry in
//! `specs/allowlist.json` that names the tracking issue.
//!
//! Run with `cargo test -p spec-check`.
//!
//! CI invokes the binary directly (see `.github/workflows/ci.yml`):
//! `cargo run -p spec-check -- --check` builds every contract crate, extracts
//! its spec, and exits non-zero on any unacknowledged breaking change.
//!
//! To (re)generate the committed baselines, run:
//! `cargo run -p spec-check -- --write`
//! which writes `specs/<crate>.json` for every contract crate.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use soroban_spec::read::from_wasm;

/// A single entry in the committed baseline / extracted spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SpecEntry {
    /// `function`, `struct`, `enum`, `error`, `event`, ...
    kind: String,
    name: String,
    /// Canonical, order-sensitive representation of the entry body.
    signature: String,
}

/// A committed baseline for one contract crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Baseline {
    crate_name: String,
    entries: Vec<SpecEntry>,
}

/// An explicit acknowledgement of a breaking change.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AllowlistEntry {
    crate_name: String,
    /// Stable identifier of the changed entry, e.g. `function:transfer`.
    entry: String,
    /// The tracking issue that authorises the break.
    issue: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Allowlist {
    #[serde(default)]
    entries: Vec<AllowlistEntry>,
}

/// Classification of a single detected change.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ChangeKind {
    /// New function/type/error/event, or a widened signature.
    Additive,
    /// Removed/renamed entry, changed argument type, reordered enum variant,
    /// changed error code, or removed struct field.
    Breaking,
}

#[derive(Debug, Clone)]
struct Change {
    kind: ChangeKind,
    entry: String,
    detail: String,
}

fn workspace_root() -> PathBuf {
    // `tools/spec-check/` -> workspace root is two levels up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("spec-check must live at tools/spec-check")
        .to_path_buf()
}

/// Extract the spec entries from a built wasm artifact.
fn extract_entries(wasm: &[u8]) -> Result<Vec<SpecEntry>, String> {
    let spec = from_wasm(wasm).map_err(|e| format!("failed to parse contractspecv0: {e}"))?;
    let mut entries = Vec::new();

    for func in &spec.functions {
        let args: Vec<String> = func
            .inputs
            .iter()
            .map(|i| format!("{}:{}", i.name, i.type_))
            .collect();
        entries.push(SpecEntry {
            kind: "function".into(),
            name: func.name.clone(),
            signature: format!("({})->{}", args.join(","), func.outputs.join(",")),
        });
    }

    for udt in &spec.udt {
        match &udt.body {
            soroban_spec::SpecEntryUdtBody::Struct(fields) => {
                // `contracttype` structs are map-encoded and sorted by name, so
                // field order is not part of the ABI. Sort for a stable diff.
                let mut sorted: Vec<String> = fields
                    .iter()
                    .map(|f| format!("{}:{}", f.name, f.type_))
                    .collect();
                sorted.sort();
                entries.push(SpecEntry {
                    kind: "struct".into(),
                    name: udt.name.clone(),
                    signature: sorted.join(","),
                });
            }
            soroban_spec::SpecEntryUdtBody::Enum(cases) => {
                // Enum variants are positional, so order is significant.
                let variants: Vec<String> = cases
                    .iter()
                    .map(|c| format!("{}:{}", c.name, c.value))
                    .collect();
                entries.push(SpecEntry {
                    kind: "enum".into(),
                    name: udt.name.clone(),
                    signature: variants.join(","),
                });
            }
            soroban_spec::SpecEntryUdtBody::Union(cases) => {
                let variants: Vec<String> = cases
                    .iter()
                    .map(|c| format!("{}:{}", c.name, c.value))
                    .collect();
                entries.push(SpecEntry {
                    kind: "union".into(),
                    name: udt.name.clone(),
                    signature: variants.join(","),
                });
            }
        }
    }

    for err in &spec.error_enums {
        let cases: Vec<String> = err
            .cases
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        entries.push(SpecEntry {
            kind: "error".into(),
            name: err.name.clone(),
            signature: cases.join(","),
        });
    }

    for event in &spec.events {
        let fields: Vec<String> = event
            .params
            .iter()
            .map(|p| format!("{}:{}", p.name, p.type_))
            .collect();
        entries.push(SpecEntry {
            kind: "event".into(),
            name: event.name.clone(),
            signature: fields.join(","),
        });
    }

    entries.sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));
    Ok(entries)
}

fn entry_id(entry: &SpecEntry) -> String {
    format!("{}:{}", entry.kind, entry.name)
}

/// Compare a baseline against a freshly extracted spec.
fn diff(baseline: &[SpecEntry], current: &[SpecEntry]) -> Vec<Change> {
    let base: BTreeMap<String, &SpecEntry> =
        baseline.iter().map(|e| (entry_id(e), e)).collect();
    let cur: BTreeMap<String, &SpecEntry> = current.iter().map(|e| (entry_id(e), e)).collect();

    let mut changes = Vec::new();

    for (id, old) in &base {
        match cur.get(id) {
            None => changes.push(Change {
                kind: ChangeKind::Breaking,
                entry: id.clone(),
                detail: "entry removed or renamed".into(),
            }),
            Some(new) if new.signature != old.signature => changes.push(Change {
                kind: ChangeKind::Breaking,
                entry: id.clone(),
                detail: format!(
                    "signature changed: `{}` -> `{}`",
                    old.signature, new.signature
                ),
            }),
            Some(_) => {}
        }
    }

    for id in cur.keys() {
        if !base.contains_key(id) {
            changes.push(Change {
                kind: ChangeKind::Additive,
                entry: id.clone(),
                detail: "new entry".into(),
            });
        }
    }

    changes
}

fn load_allowlist(root: &Path) -> Result<Allowlist, String> {
    let path = root.join("specs/allowlist.json");
    match fs::read_to_string(&path) {
        Ok(raw) => {
            serde_json::from_str(&raw).map_err(|e| format!("invalid {}: {e}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Allowlist::default()),
        Err(e) => Err(format!("failed to read {}: {e}", path.display())),
    }
}

fn is_allowed(allowlist: &Allowlist, crate_name: &str, entry: &str) -> bool {
    allowlist.entries.iter().any(|a| {
        a.crate_name == crate_name && a.entry == entry && !a.issue.trim().is_empty()
    })
}

/// Discover every contract crate that has a committed baseline.
fn baseline_crates(root: &Path) -> Vec<(String, PathBuf)> {
    let specs_dir = root.join("specs");
    let mut crates = Vec::new();
    let Ok(read) = fs::read_dir(&specs_dir) else {
        return crates;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // `allowlist.json` is not a crate baseline.
        if stem == "allowlist" {
            continue;
        }
        crates.push((stem.to_string(), path));
    }
    crates.sort();
    crates
}

/// Build a contract crate and return the path to its wasm artifact.
fn build_wasm(root: &Path, crate_name: &str) -> Result<PathBuf, String> {
    let status = Command::new("cargo")
        .current_dir(root)
        .args(["build", "-p", crate_name, "--target", "wasm32-unknown-unknown", "--release"])
        .status()
        .map_err(|e| format!("failed to spawn cargo for {crate_name}: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build failed for {crate_name}"));
    }
    let wasm = root
        .join("target/wasm32-unknown-unknown/release")
        .join(format!("{}.wasm", crate_name.replace('-', "_")));
    if !wasm.exists() {
        return Err(format!("wasm artifact not found at {}", wasm.display()));
    }
    Ok(wasm)
}

/// Write a baseline for a single crate to `specs/<crate>.json`.
fn write_baseline(root: &Path, crate_name: &str) -> Result<(), String> {
    let wasm = build_wasm(root, crate_name)?;
    let bytes = fs::read(&wasm).map_err(|e| format!("failed to read {}: {e}", wasm.display()))?;
    let entries = extract_entries(&bytes)?;
    let baseline = Baseline {
        crate_name: crate_name.to_string(),
        entries,
    };
    let specs_dir = root.join("specs");
    fs::create_dir_all(&specs_dir)
        .map_err(|e| format!("failed to create {}: {e}", specs_dir.display()))?;
    let path = specs_dir.join(format!("{crate_name}.json"));
    let json = serde_json::to_string_pretty(&baseline)
        .map_err(|e| format!("failed to serialize baseline: {e}"))?;
    fs::write(&path, format!("{json}\n"))
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// Check a single crate's current spec against its committed baseline.
fn check_crate(root: &Path, crate_name: &str, baseline_path: &Path) -> Result<bool, String> {
    let raw = fs::read_to_string(baseline_path)
        .map_err(|e| format!("failed to read {}: {e}", baseline_path.display()))?;
    let baseline: Baseline = serde_json::from_str(&raw)
        .map_err(|e| format!("invalid {}: {e}", baseline_path.display()))?;

    let wasm = build_wasm(root, crate_name)?;
    let bytes = fs::read(&wasm).map_err(|e| format!("failed to read {}: {e}", wasm.display()))?;
    let current = extract_entries(&bytes)?;

    let allowlist = load_allowlist(root)?;
    let changes = diff(&baseline.entries, &current);

    let mut ok = true;
    for change in &changes {
        match change.kind {
            ChangeKind::Additive => {
                println!("[additive] {crate_name} {}: {}", change.entry, change.detail);
            }
            ChangeKind::Breaking => {
                if is_allowed(&allowlist, crate_name, &change.entry) {
                    println!("[allowed]  {crate_name} {}: {}", change.entry, change.detail);
                } else {
                    ok = false;
                    eprintln!("[breaking] {crate_name} {}: {}", change.entry, change.detail);
                }
            }
        }
    }
    Ok(ok)
}

fn main() {
    let root = workspace_root();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let write = args.iter().any(|a| a == "--write");

    if write {
        // Regenerate baselines for every contract crate in the workspace.
        let crates = contract_crates(&root);
        if crates.is_empty() {
            eprintln!("no contract crates found");
            std::process::exit(1);
        }
        for crate_name in &crates {
            if let Err(e) = write_baseline(&root, crate_name) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    let crates = baseline_crates(&root);
    if crates.is_empty() {
        eprintln!("no baselines found in specs/; run `cargo run -p spec-check -- --write`");
        std::process::exit(1);
    }

    let mut all_ok = true;
    for (crate_name, path) in &crates {
        match check_crate(&root, crate_name, path) {
            Ok(ok) => all_ok &= ok,
            Err(e) => {
                eprintln!("error: {e}");
                all_ok = false;
            }
        }
    }

    if !all_ok {
        eprintln!("\nspec-check failed: unacknowledged breaking changes detected");
        std::process::exit(1);
    }
    println!("spec-check passed");
}

/// Discover every contract crate in the workspace (members with a `lib` target
/// that produce a wasm artifact).
fn contract_crates(root: &Path) -> Vec<String> {
    let mut crates = Vec::new();
    let Ok(read) = fs::read_dir(root) else {
        return crates;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join("Cargo.toml");
        if !manifest.exists() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&manifest) else {
            continue;
        };
        // Contract crates depend on `soroban-sdk`.
        if !raw.contains("soroban-sdk") {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            crates.push(name.to_string());
        }
    }
    crates.sort();
    crates
}
