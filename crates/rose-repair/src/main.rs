//! rose-repair: run LTK Manager's "Repair" on one mod archive, headless.
//!
//! usage: rose-repair <mod> <out> --league <League of Legends dir>
//!
//! The mod is installed into a throwaway library, repaired there with
//! LTK Manager's library crates, and exported to <out>. The last stdout line is JSON:
//! {"status": "repaired"|"unchanged"|"failed", "applied": n, "extension": "...", "error": "..."}

use ltk_manager_assets::hashtables::{HashtableCache, WadPathResolverState};
use ltk_manager_base::config::Config;
use ltk_manager_base::events::NullEventSink;
use ltk_manager_library::mods::{
    ChecksumMismatchState, ExportScope, ExportShape, LinkedBinState, ModLibrary, WadReportState,
};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const USER_AGENT: &str = "Rose-Repair/0.1 (+https://github.com/Alban1911/Rose)";
const USAGE: &str = "usage: rose-repair <mod> <out> --league <League of Legends dir>";

fn main() {
    let report = match run() {
        Ok(report) => report,
        Err(error) => json!({ "status": "failed", "applied": 0, "error": error }),
    };
    println!("{report}");
}

fn run() -> Result<serde_json::Value, String> {
    let mut args = std::env::args().skip(1);
    let input = PathBuf::from(args.next().ok_or(USAGE)?);
    let output = PathBuf::from(args.next().ok_or(USAGE)?);
    let mut league = None;
    while let Some(arg) = args.next() {
        if arg == "--league" {
            league = args.next().map(PathBuf::from);
        }
    }

    // The repair names bin and WAD hashes with the shared hashtable cache,
    // the same one LTK Manager syncs; fetch or update it first.
    let cache =
        HashtableCache::shared().map_err(|e| format!("hashtables are not available: {e}"))?;
    if let Err(e) = cache.sync(false, USER_AGENT, &NullEventSink) {
        eprintln!("hashtable sync: {e}");
    }

    let work = std::env::temp_dir().join(format!("rose-repair-{}", std::process::id()));
    let result = repair(&input, &output, league, &work);
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn repair(
    input: &Path,
    output: &Path,
    league: Option<PathBuf>,
    work: &Path,
) -> Result<serde_json::Value, String> {
    let storage = work.join("library");
    std::fs::create_dir_all(&storage).map_err(|e| e.to_string())?;
    let config = Config {
        league_path: league,
        mod_storage_path: Some(storage.clone()),
        ..Config::default()
    };
    let library = ModLibrary::new(
        Arc::new(NullEventSink),
        Some(storage.clone()),
        env!("CARGO_PKG_VERSION"),
        Arc::new(LinkedBinState::default()),
        Arc::new(ChecksumMismatchState::default()),
        Arc::new(WadReportState::new(Some(&storage))),
        Arc::new(WadPathResolverState::default()),
    );

    let installed = library
        .install_mod_from_package(&config, &input.to_string_lossy())
        .map_err(|e| e.to_string())?
        .into_mod();
    let fixes = library
        .repair_mod(&config, &installed.id)
        .map_err(|e| e.to_string())?;
    if fixes.applied == 0 {
        return Ok(json!({ "status": "unchanged", "applied": 0 }));
    }

    let exported = work.join("export");
    library
        .export_mods(&config, ExportScope::All, ExportShape::Folder, &exported)
        .map_err(|e| e.to_string())?;
    let file = std::fs::read_dir(&exported)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.is_file())
        .ok_or("repaired archive not found")?;
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(&file, output).map_err(|e| e.to_string())?;
    let extension = file
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(json!({ "status": "repaired", "applied": fixes.applied, "extension": extension }))
}
