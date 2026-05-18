// Codegen build script for hexo-engine.
//
// Reads ../hexo.toml (single source of truth for engine config) and emits
// $OUT_DIR/config_generated.rs containing `pub const` definitions referenced
// by src/config.rs. See SPEC_CONFIG.md.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = Path::new(&manifest_dir).parent().unwrap();
    let cfg_path = workspace_root.join("hexo.toml");

    println!("cargo:rerun-if-changed={}", cfg_path.display());

    let text = fs::read_to_string(&cfg_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", cfg_path.display()));
    let cfg: toml::Value = toml::from_str(&text)
        .unwrap_or_else(|e| panic!("invalid hexo.toml: {e}"));

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let out_path: PathBuf = Path::new(&out_dir).join("config_generated.rs");

    let mut out = String::new();
    out.push_str("// AUTO-GENERATED from hexo.toml — do not edit.\n\n");

    emit_i32(&mut out, &cfg, &["engine", "eval", "mate_score"], "MATE_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "open_5"], "OPEN_5_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "closed_5"], "CLOSED_5_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "open_4"], "OPEN_4_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "closed_4"], "CLOSED_4_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "open_3"], "OPEN_3_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "rhombus"], "RHOMBUS_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "arch"], "ARCH_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "bone"], "BONE_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "trapezoid"], "TRAPEZOID_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "open_2"], "OPEN_2_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "closed_3"], "CLOSED_3_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "triangle"], "TRIANGLE_SCORE");
    emit_i32(&mut out, &cfg, &["engine", "eval", "overlap_bonus_x10"], "OVERLAP_BONUS_X10");

    emit_i32_array(
        &mut out,
        &cfg,
        &["engine", "eval", "window_k_scores"],
        "WINDOW_K_SCORES",
        7,
    );

    emit_usize(&mut out, &cfg, &["engine", "search", "default_max_depth"], "DEFAULT_MAX_DEPTH");
    emit_u64(&mut out, &cfg, &["engine", "search", "default_time_ms"], "DEFAULT_TIME_MS");
    emit_usize(&mut out, &cfg, &["engine", "search", "default_tt_size_mb"], "DEFAULT_TT_SIZE_MB");
    emit_i16(&mut out, &cfg, &["engine", "search", "default_move_radius"], "DEFAULT_MOVE_RADIUS");
    emit_i16(&mut out, &cfg, &["engine", "search", "extended_move_radius"], "EXTENDED_MOVE_RADIUS");
    emit_i16(&mut out, &cfg, &["engine", "search", "full_legality_radius"], "FULL_LEGALITY_RADIUS");
    emit_usize(&mut out, &cfg, &["engine", "search", "move_cap"], "MOVE_CAP");
    emit_u64(&mut out, &cfg, &["engine", "search", "deadline_check_nodes"], "DEADLINE_CHECK_NODES");
    emit_i8(&mut out, &cfg, &["engine", "search", "aspiration_start_depth"], "ASPIRATION_START_DEPTH");

    emit_i16(&mut out, &cfg, &["engine", "board", "max_piece_distance"], "MAX_PIECE_DISTANCE");
    emit_i16(&mut out, &cfg, &["engine", "board", "zobrist_window"], "ZOBRIST_WINDOW");

    fs::write(&out_path, out).expect("write config_generated.rs");
}

fn get<'a>(cfg: &'a toml::Value, path: &[&str]) -> &'a toml::Value {
    let mut cur = cfg;
    for key in path {
        cur = cur.get(*key).unwrap_or_else(|| panic!("hexo.toml missing {}", path.join(".")));
    }
    cur
}

fn as_int(v: &toml::Value, path: &[&str]) -> i64 {
    v.as_integer()
        .unwrap_or_else(|| panic!("hexo.toml {} not an integer", path.join(".")))
}

fn emit_i32(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    out.push_str(&format!("pub const {name}: i32 = {v};\n"));
}

fn emit_i16(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    out.push_str(&format!("pub const {name}: i16 = {v};\n"));
}

fn emit_i8(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    out.push_str(&format!("pub const {name}: i8 = {v};\n"));
}

fn emit_u64(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    out.push_str(&format!("pub const {name}: u64 = {v};\n"));
}

fn emit_usize(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    out.push_str(&format!("pub const {name}: usize = {v};\n"));
}

fn emit_i32_array(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str, len: usize) {
    let arr = get(cfg, path)
        .as_array()
        .unwrap_or_else(|| panic!("hexo.toml {} not an array", path.join(".")));
    assert_eq!(arr.len(), len, "hexo.toml {} must have {len} entries", path.join("."));
    let body = arr
        .iter()
        .map(|v| v.as_integer().expect("array item not int").to_string())
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("pub const {name}: [i32; {len}] = [{body}];\n"));
}
