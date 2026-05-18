// Codegen build script for hexo-engine.
//
// Reads ../hexo.toml (single source of truth for engine config) and emits
// $OUT_DIR/config_generated.rs containing `pub const` definitions referenced
// by src/config.rs. See SPEC_CONFIG.md.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = Path::new(&manifest_dir).parent().unwrap();
    let cfg_path = workspace_root.join("hexo.toml");

    println!("cargo:rerun-if-changed={}", cfg_path.display());

    let text = fs::read_to_string(&cfg_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", cfg_path.display()));
    let cfg: toml::Value =
        toml::from_str(&text).unwrap_or_else(|e| panic!("invalid hexo.toml: {e}"));

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let out_path: PathBuf = Path::new(&out_dir).join("config_generated.rs");

    let mut out = String::new();
    out.push_str("// AUTO-GENERATED from hexo.toml — do not edit.\n\n");

    emit_eval(&mut out, &cfg);
    emit_threats(&mut out, &cfg);
    emit_tt(&mut out, &cfg);
    emit_search(&mut out, &cfg);
    emit_ordering(&mut out, &cfg);
    emit_board(&mut out, &cfg);

    fs::write(&out_path, out).expect("write config_generated.rs");
}

fn emit_eval(out: &mut String, cfg: &toml::Value) {
    let scalars: &[(&[&str], &str)] = &[
        (&["engine", "eval", "mate_score"], "MATE_SCORE"),
        (&["engine", "eval", "open_5"], "OPEN_5_SCORE"),
        (&["engine", "eval", "closed_5"], "CLOSED_5_SCORE"),
        (&["engine", "eval", "open_4"], "OPEN_4_SCORE"),
        (&["engine", "eval", "closed_4"], "CLOSED_4_SCORE"),
        (&["engine", "eval", "open_3"], "OPEN_3_SCORE"),
        (&["engine", "eval", "rhombus"], "RHOMBUS_SCORE"),
        (&["engine", "eval", "arch"], "ARCH_SCORE"),
        (&["engine", "eval", "bone"], "BONE_SCORE"),
        (&["engine", "eval", "trapezoid"], "TRAPEZOID_SCORE"),
        (&["engine", "eval", "open_2"], "OPEN_2_SCORE"),
        (&["engine", "eval", "closed_3"], "CLOSED_3_SCORE"),
        (&["engine", "eval", "triangle"], "TRIANGLE_SCORE"),
        (
            &["engine", "eval", "overlap_bonus_x10"],
            "OVERLAP_BONUS_X10",
        ),
        (
            &["engine", "eval", "open_extension_factor"],
            "OPEN_EXTENSION_FACTOR",
        ),
        (
            &["engine", "eval", "closed_extension_factor"],
            "CLOSED_EXTENSION_FACTOR",
        ),
        (
            &["engine", "eval", "fork_cover2_bonus"],
            "FORK_COVER2_BONUS",
        ),
        (&["engine", "eval", "tempo_weight"], "TEMPO_WEIGHT"),
    ];
    for (path, name) in scalars {
        emit_i32(out, cfg, path, name);
    }
    emit_i32_array(
        out,
        cfg,
        &["engine", "eval", "window_k_scores"],
        "WINDOW_K_SCORES",
        7,
    );
    emit_window_score_table(out, cfg);
}

/// Emit `WINDOW_SCORE: [i32; 729]` for Layer 1 ternary-encoded windows.
///
/// Index = `c0 + 3*c1 + 9*c2 + 27*c3 + 81*c4 + 243*c5` with cell codes
/// `0=empty, 1=X, 2=O`. Mixed windows → 0. X-only → `+k_scores[k]`.
/// O-only → `-k_scores[k]`. Empty → 0.
fn emit_window_score_table(out: &mut String, cfg: &toml::Value) {
    let path: &[&str] = &["engine", "eval", "window_k_scores"];
    let arr = get(cfg, path)
        .as_array()
        .expect("window_k_scores must be array");
    assert_eq!(arr.len(), 7, "window_k_scores must have 7 entries");
    let k_scores: Vec<i32> = arr
        .iter()
        .map(|v| {
            i32::try_from(v.as_integer().expect("k score not int"))
                .expect("k score does not fit in i32")
        })
        .collect();

    // Invariant: a 6-in-window is a win and must match mate_score so
    // Layer 1 and the terminal-position short-circuit agree.
    let mate = i32::try_from(as_int(
        get(cfg, &["engine", "eval", "mate_score"]),
        &["engine", "eval", "mate_score"],
    ))
    .expect("mate_score does not fit in i32");
    assert_eq!(
        k_scores[6], mate,
        "window_k_scores[6] must equal mate_score; got {} vs {}",
        k_scores[6], mate
    );

    let mut entries: Vec<i32> = Vec::with_capacity(729);
    for idx in 0..729u16 {
        let mut x_count: u8 = 0;
        let mut o_count: u8 = 0;
        let mut n = idx;
        for _ in 0..6 {
            let cell = n % 3;
            n /= 3;
            match cell {
                1 => x_count += 1,
                2 => o_count += 1,
                _ => {}
            }
        }
        let v = if x_count > 0 && o_count > 0 {
            0
        } else if x_count > 0 {
            k_scores[x_count as usize]
        } else if o_count > 0 {
            -k_scores[o_count as usize]
        } else {
            0
        };
        entries.push(v);
    }
    let body = entries
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(out, "pub const WINDOW_SCORE: [i32; 729] = [{body}];").unwrap();
}

fn emit_threats(out: &mut String, cfg: &toml::Value) {
    emit_i16(
        out,
        cfg,
        &["engine", "threats", "recompute_radius"],
        "THREAT_RECOMPUTE_RADIUS",
    );
    emit_i16(
        out,
        cfg,
        &["engine", "threats", "cluster_radius"],
        "THREAT_CLUSTER_RADIUS",
    );
    emit_usize(
        out,
        cfg,
        &["engine", "threats", "max_s0_instances_per_player"],
        "MAX_S0_INSTANCES",
    );
}

fn emit_tt(out: &mut String, cfg: &toml::Value) {
    emit_usize(
        out,
        cfg,
        &["engine", "tt", "default_size_mb"],
        "DEFAULT_TT_SIZE_MB",
    );
}

fn emit_search(out: &mut String, cfg: &toml::Value) {
    emit_usize(
        out,
        cfg,
        &["engine", "search", "default_max_depth"],
        "DEFAULT_MAX_DEPTH",
    );
    emit_u64(
        out,
        cfg,
        &["engine", "search", "default_time_ms"],
        "DEFAULT_TIME_MS",
    );
    emit_i16(
        out,
        cfg,
        &["engine", "search", "default_move_radius"],
        "DEFAULT_MOVE_RADIUS",
    );
    emit_i16(
        out,
        cfg,
        &["engine", "search", "extended_move_radius"],
        "EXTENDED_MOVE_RADIUS",
    );
    emit_i16(
        out,
        cfg,
        &["engine", "search", "full_legality_radius"],
        "FULL_LEGALITY_RADIUS",
    );
    emit_usize(out, cfg, &["engine", "search", "move_cap"], "MOVE_CAP");
    emit_u64(
        out,
        cfg,
        &["engine", "search", "deadline_check_nodes"],
        "DEADLINE_CHECK_NODES",
    );
    emit_i8(
        out,
        cfg,
        &["engine", "search", "aspiration_start_depth"],
        "ASPIRATION_START_DEPTH",
    );
    emit_i16(
        out,
        cfg,
        &["engine", "search", "move_gen_inner_radius"],
        "MOVE_GEN_INNER_RADIUS",
    );
    emit_i16(
        out,
        cfg,
        &["engine", "search", "move_gen_outer_radius"],
        "MOVE_GEN_OUTER_RADIUS",
    );
}

fn emit_ordering(out: &mut String, cfg: &toml::Value) {
    emit_usize(
        out,
        cfg,
        &["engine", "ordering", "move_gen_cap"],
        "MOVE_GEN_CAP",
    );
    emit_usize(
        out,
        cfg,
        &["engine", "ordering", "killer_slots"],
        "KILLER_SLOTS",
    );
    emit_usize(out, cfg, &["engine", "ordering", "max_ply"], "MAX_PLY");
    emit_u32(
        out,
        cfg,
        &["engine", "ordering", "history_cutoff_max"],
        "HISTORY_CUTOFF_MAX",
    );
    emit_u32(
        out,
        cfg,
        &["engine", "ordering", "history_decay_num"],
        "HISTORY_DECAY_NUM",
    );
    emit_u32(
        out,
        cfg,
        &["engine", "ordering", "history_decay_den"],
        "HISTORY_DECAY_DEN",
    );
}

fn emit_board(out: &mut String, cfg: &toml::Value) {
    emit_i16(
        out,
        cfg,
        &["engine", "board", "max_piece_distance"],
        "MAX_PIECE_DISTANCE",
    );
    emit_i16(
        out,
        cfg,
        &["engine", "board", "zobrist_window"],
        "ZOBRIST_WINDOW",
    );
}

fn get<'a>(cfg: &'a toml::Value, path: &[&str]) -> &'a toml::Value {
    let mut cur = cfg;
    for key in path {
        cur = cur
            .get(*key)
            .unwrap_or_else(|| panic!("hexo.toml missing {}", path.join(".")));
    }
    cur
}

fn as_int(v: &toml::Value, path: &[&str]) -> i64 {
    v.as_integer()
        .unwrap_or_else(|| panic!("hexo.toml {} not an integer", path.join(".")))
}

fn emit_i32(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    writeln!(out, "pub const {name}: i32 = {v};").unwrap();
}

fn emit_i16(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    writeln!(out, "pub const {name}: i16 = {v};").unwrap();
}

fn emit_i8(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    writeln!(out, "pub const {name}: i8 = {v};").unwrap();
}

fn emit_u64(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    writeln!(out, "pub const {name}: u64 = {v};").unwrap();
}

fn emit_u32(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    writeln!(out, "pub const {name}: u32 = {v};").unwrap();
}

fn emit_usize(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    writeln!(out, "pub const {name}: usize = {v};").unwrap();
}

fn emit_i32_array(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str, len: usize) {
    let arr = get(cfg, path)
        .as_array()
        .unwrap_or_else(|| panic!("hexo.toml {} not an array", path.join(".")));
    assert_eq!(
        arr.len(),
        len,
        "hexo.toml {} must have {len} entries",
        path.join(".")
    );
    let body = arr
        .iter()
        .map(|v| v.as_integer().expect("array item not int").to_string())
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(out, "pub const {name}: [i32; {len}] = [{body}];").unwrap();
}
