// Bench tooling — pedantic style lints add noise without value.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::map_unwrap_or,
    clippy::let_and_return,
    clippy::needless_pass_by_value,
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::redundant_closure_for_method_calls,
    clippy::manual_let_else,
    clippy::single_match_else,
    clippy::too_many_lines
)]
//! Consolidates criterion's per-bench `estimates.json` into a single
//! canonical JSON file per the `SPEC_BENCHMARKS.md` schema.
//!
//! Walks `target/criterion/<group>/<bench>/new/estimates.json` for every
//! group + bench pair, extracts median + MAD + sample count, then writes
//! the consolidated record to `benches/results/<isodate>-<sha>.json`.
//!
//! Exit code is nonzero on any I/O or parse failure; no panics in the
//! happy path. CLI:
//!
//! ```text
//! bench_drain --out PATH [--criterion-dir DIR]
//! ```

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use chrono::Utc;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = hexo_engine::config::BENCH_SCHEMA_VERSION;

#[derive(Debug, Deserialize)]
struct EstimateInner {
    point_estimate: f64,
}

#[derive(Debug, Deserialize)]
struct Estimates {
    median: EstimateInner,
    median_abs_dev: EstimateInner,
}

#[derive(Debug, Deserialize)]
struct SampleSummary {
    #[serde(default)]
    times: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct BenchmarkMeta {
    #[serde(default)]
    group_id: String,
    #[serde(default)]
    function_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct BenchRecord {
    group: String,
    name: String,
    median_ns: f64,
    mad_ns: f64,
    samples: usize,
}

#[derive(Debug, Serialize)]
struct HostInfo {
    cpu: String,
    cores: usize,
}

#[derive(Debug, Serialize)]
struct Drain {
    schema_version: u32,
    timestamp: String,
    git_sha: String,
    rustc_version: String,
    host: HostInfo,
    /// Per-bench records. Named `micro` to match the canonical schema's
    /// micro/macro split — `bench-micro` output is itself a canonical
    /// JSON with `macro` left empty, so the diff tool can join it
    /// against any other canonical run.
    micro: Vec<BenchRecord>,
    /// Empty placeholder — canonical schema expects this key. The
    /// Python CLI's `bench all` overwrites it with macro-bench results.
    #[serde(rename = "macro")]
    macro_: serde_json::Value,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(io::stderr(), "bench_drain: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    let mut out_path: Option<PathBuf> = None;
    let mut crit_dir: PathBuf = PathBuf::from("target/criterion");
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out_path = Some(PathBuf::from(args.get(i).ok_or("--out needs PATH")?));
            }
            "--criterion-dir" => {
                i += 1;
                crit_dir = PathBuf::from(args.get(i).ok_or("--criterion-dir needs DIR")?);
            }
            other => return Err(format!("unknown arg: {other}")),
        }
        i += 1;
    }

    let sha = git_sha();
    let rustc = rustc_version();
    let host = host_info();
    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let micro = scan_criterion(&crit_dir)?;
    let drain = Drain {
        schema_version: SCHEMA_VERSION,
        timestamp: timestamp.clone(),
        git_sha: sha.clone(),
        rustc_version: rustc,
        host,
        micro,
        macro_: serde_json::json!({
            "nps": [],
            "depth_at_time": [],
            "threat_latency": [],
            "selfplay_throughput": [],
        }),
    };

    let path = out_path.unwrap_or_else(|| {
        let date = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        PathBuf::from(format!("benches/results/{date}-{sha}.json"))
    });
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir_all {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(&drain)
        .map_err(|e| format!("serialize: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))?;

    println!("{}", path.display());
    Ok(())
}

fn git_sha() -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn host_info() -> HostInfo {
    let cores = std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(0);
    let cpu = read_cpu_model();
    HostInfo { cpu, cores }
}

fn read_cpu_model() -> String {
    let text = match fs::read_to_string("/proc/cpuinfo") {
        Ok(t) => t,
        Err(_) => return String::new(),
    };
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':')
            && k.trim() == "model name"
        {
            return v.trim().to_string();
        }
    }
    String::new()
}

/// Walk `crit_dir` for `<group>/<bench>/new/{estimates.json,benchmark.json}`.
fn scan_criterion(crit_dir: &Path) -> Result<Vec<BenchRecord>, String> {
    let mut out = Vec::new();
    if !crit_dir.is_dir() {
        return Ok(out);
    }
    for entry in fs::read_dir(crit_dir).map_err(|e| format!("read_dir: {e}"))? {
        let group_entry = entry.map_err(|e| format!("entry: {e}"))?;
        let group_path = group_entry.path();
        if !group_path.is_dir() {
            continue;
        }
        let group_name = group_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if group_name == "report" {
            continue;
        }
        scan_group(&group_path, &group_name, &mut out)?;
    }
    out.sort_by(|a, b| a.group.cmp(&b.group).then(a.name.cmp(&b.name)));
    Ok(out)
}

fn scan_group(
    group_path: &Path,
    group_name: &str,
    out: &mut Vec<BenchRecord>,
) -> Result<(), String> {
    for entry in fs::read_dir(group_path).map_err(|e| format!("read_dir: {e}"))? {
        let bench_entry = entry.map_err(|e| format!("entry: {e}"))?;
        let bench_path = bench_entry.path();
        if !bench_path.is_dir() {
            continue;
        }
        let bench_name = bench_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if bench_name == "report" {
            continue;
        }
        let new_dir = bench_path.join("new");
        let est_path = new_dir.join("estimates.json");
        if !est_path.is_file() {
            continue;
        }
        let est_txt = fs::read_to_string(&est_path)
            .map_err(|e| format!("read {}: {e}", est_path.display()))?;
        let est: Estimates = serde_json::from_str(&est_txt)
            .map_err(|e| format!("parse {}: {e}", est_path.display()))?;
        let (canonical_group, canonical_name) =
            read_canonical_ids(&new_dir, group_name, &bench_name);
        let samples = read_sample_count(&new_dir);
        out.push(BenchRecord {
            group: canonical_group,
            name: canonical_name,
            median_ns: est.median.point_estimate,
            mad_ns: est.median_abs_dev.point_estimate,
            samples,
        });
    }
    Ok(())
}

/// Pull the criterion-recorded `group_id` / `function_id` from
/// `new/benchmark.json` so the canonical output shows the original
/// names (e.g. `threats::compute`) rather than the filesystem-safe
/// directory names (`threats__compute`).
fn read_canonical_ids(
    new_dir: &Path,
    fallback_group: &str,
    fallback_name: &str,
) -> (String, String) {
    let path = new_dir.join("benchmark.json");
    let Ok(txt) = fs::read_to_string(&path) else {
        return (fallback_group.to_string(), fallback_name.to_string());
    };
    let Ok(meta) = serde_json::from_str::<BenchmarkMeta>(&txt) else {
        return (fallback_group.to_string(), fallback_name.to_string());
    };
    let group = if meta.group_id.is_empty() {
        fallback_group.to_string()
    } else {
        meta.group_id
    };
    let name = meta
        .function_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback_name.to_string());
    (group, name)
}

/// Sample count comes from `new/sample.json` (`times` array length).
/// `new/benchmark.json` carries only metadata, not sample data.
fn read_sample_count(new_dir: &Path) -> usize {
    let path = new_dir.join("sample.json");
    let Ok(txt) = fs::read_to_string(&path) else {
        return 0;
    };
    let Ok(s) = serde_json::from_str::<SampleSummary>(&txt) else {
        return 0;
    };
    s.times.len()
}
