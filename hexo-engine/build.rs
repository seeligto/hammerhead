// Codegen build script for hexo-engine.
//
// Reads ../hexo.toml (single source of truth for engine config) and emits
// $OUT_DIR/config_generated.rs containing `pub const` definitions referenced
// by src/config.rs. See SPEC_CONFIG.md.

// Build script is dev tooling; pedantic style lints add noise without value.
#![allow(
    clippy::needless_continue,
    clippy::manual_assert,
    clippy::cast_possible_truncation,
    clippy::map_unwrap_or,
    clippy::needless_lifetimes,
    clippy::elidable_lifetime_names,
    clippy::doc_lazy_continuation,
    clippy::doc_overindented_list_items
)]

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
    emit_bench(&mut out, &cfg);

    fs::write(&out_path, out).expect("write config_generated.rs");

    // Codegen fixture builders from benches/fixtures/positions.json. Emits a
    // second file consumed by benches/common/positions.rs.
    let fx_rel = cfg
        .get("bench")
        .and_then(|b| b.get("fixtures_path"))
        .and_then(|v| v.as_str())
        .unwrap_or("benches/fixtures/positions.json");
    let fx_path = workspace_root.join(fx_rel);
    println!("cargo:rerun-if-changed={}", fx_path.display());
    emit_fixtures(&fx_path, &out_dir);
}

fn emit_bench(out: &mut String, cfg: &toml::Value) {
    emit_u64(
        out,
        cfg,
        &["bench", "default_time_ms"],
        "BENCH_DEFAULT_TIME_MS",
    );
    emit_u32(
        out,
        cfg,
        &["bench", "schema_version"],
        "BENCH_SCHEMA_VERSION",
    );
    // Phase 12: reference node-count config.
    emit_u32(
        out,
        cfg,
        &["bench", "reference", "max_depth"],
        "BENCH_REFERENCE_MAX_DEPTH",
    );
    emit_u32(
        out,
        cfg,
        &["bench", "reference", "budget_s"],
        "BENCH_REFERENCE_BUDGET_S",
    );
}

/// Codegen fixture builders from `positions.json` → `$OUT_DIR/fixtures_generated.rs`.
///
/// The JSON maps fixture name → `{ "moves": [[q,r], ...] }`. We emit:
/// - one `pub(crate) fn build_<name>() -> Board` per entry, applying each
///   move via `Board::place_for_test` with X/O alternating by ply parity.
/// - `pub(crate) static FIXTURE_TABLE: &[(name, fn() -> Board)]` for iteration.
/// Emits `$OUT_DIR/fixtures_generated.rs` for inclusion by
/// `benches/common/positions.rs`. The including module must define a
/// `Fixture` type with `name: &'static str` and `build: fn() -> Board`
/// fields and have `Board`, `Coord`, and `player_at_ply` in scope.
fn emit_fixtures(fx_path: &Path, out_dir: &std::ffi::OsStr) {
    let mut out = String::new();
    out.push_str("// AUTO-GENERATED from benches/fixtures/positions.json — do not edit.\n\n");

    let entries: Vec<(String, Vec<(i16, i16)>)> = if fx_path.is_file() {
        let txt = fs::read_to_string(fx_path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", fx_path.display()));
        parse_fixtures_json(&parse_json(&txt))
    } else {
        Vec::new()
    };

    let mut names: Vec<String> = Vec::new();
    for (name, moves) in &entries {
        if !is_snake_ident(name) {
            panic!("fixture name {name:?} must be lowercase_snake");
        }
        names.push(name.clone());
        writeln!(out, "pub fn build_{name}() -> Board {{").unwrap();
        let binding = if moves.is_empty() { "let b" } else { "let mut b" };
        writeln!(out, "    {binding} = Board::new();").unwrap();
        for (i, (q, r)) in moves.iter().enumerate() {
            writeln!(
                out,
                "    b.place_for_test(Coord::new({q}, {r}), player_at_ply({i}));",
            )
            .unwrap();
        }
        out.push_str("    b\n");
        out.push_str("}\n\n");
    }

    out.push_str("pub static FIXTURES: &[Fixture] = &[\n");
    for name in &names {
        writeln!(
            out,
            "    Fixture {{ name: \"{name}\", build: build_{name} }},",
        )
        .unwrap();
    }
    out.push_str("];\n");

    let out_path: PathBuf = Path::new(out_dir).join("fixtures_generated.rs");
    fs::write(&out_path, out).expect("write fixtures_generated.rs");
}

/// Tiny JSON parser via `toml::Value`. Avoids pulling in `serde_json` as a
/// build-dep when we only need `{string -> {moves -> [[int,int]]}}`. The
/// fixtures file is hand-written and small.
fn parse_json(s: &str) -> JsonValue {
    let mut p = JsonParser { s: s.as_bytes(), pos: 0 };
    p.skip_ws();
    let v = p.parse_value();
    p.skip_ws();
    if p.pos != p.s.len() {
        panic!("fixtures JSON: trailing junk at byte {}", p.pos);
    }
    v
}

#[derive(Debug)]
#[allow(dead_code)] // Bool / Null appear in the grammar but the schema never reads them.
enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

struct JsonParser<'a> {
    s: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn peek(&self) -> u8 {
        if self.pos >= self.s.len() {
            panic!("fixtures JSON: unexpected EOF");
        }
        self.s[self.pos]
    }
    fn bump(&mut self) -> u8 {
        let b = self.peek();
        self.pos += 1;
        b
    }
    fn skip_ws(&mut self) {
        while self.pos < self.s.len()
            && matches!(self.s[self.pos], b' ' | b'\t' | b'\n' | b'\r')
        {
            self.pos += 1;
        }
    }
    fn expect(&mut self, c: u8) {
        let b = self.bump();
        if b != c {
            panic!(
                "fixtures JSON: expected {:?} got {:?} at byte {}",
                c as char, b as char, self.pos - 1
            );
        }
    }
    fn parse_value(&mut self) -> JsonValue {
        self.skip_ws();
        match self.peek() {
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b'"' => JsonValue::String(self.parse_string()),
            b't' | b'f' => self.parse_bool(),
            b'n' => self.parse_null(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            c => panic!("fixtures JSON: unexpected byte {:?} at {}", c as char, self.pos),
        }
    }
    fn parse_object(&mut self) -> JsonValue {
        self.expect(b'{');
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek() == b'}' {
            self.pos += 1;
            return JsonValue::Object(entries);
        }
        loop {
            self.skip_ws();
            let k = self.parse_string();
            self.skip_ws();
            self.expect(b':');
            let v = self.parse_value();
            entries.push((k, v));
            self.skip_ws();
            match self.bump() {
                b',' => continue,
                b'}' => break,
                c => panic!("fixtures JSON: expected ',' or '}}' got {:?}", c as char),
            }
        }
        JsonValue::Object(entries)
    }
    fn parse_array(&mut self) -> JsonValue {
        self.expect(b'[');
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == b']' {
            self.pos += 1;
            return JsonValue::Array(items);
        }
        loop {
            items.push(self.parse_value());
            self.skip_ws();
            match self.bump() {
                b',' => continue,
                b']' => break,
                c => panic!("fixtures JSON: expected ',' or ']' got {:?}", c as char),
            }
        }
        JsonValue::Array(items)
    }
    fn parse_string(&mut self) -> String {
        self.expect(b'"');
        let mut out = String::new();
        loop {
            let b = self.bump();
            match b {
                b'"' => return out,
                b'\\' => {
                    let e = self.bump();
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        c => panic!("fixtures JSON: unsupported escape \\{}", c as char),
                    }
                }
                c => out.push(c as char),
            }
        }
    }
    fn parse_number(&mut self) -> JsonValue {
        let start = self.pos;
        if self.peek() == b'-' {
            self.pos += 1;
        }
        while self.pos < self.s.len()
            && matches!(self.s[self.pos], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        {
            self.pos += 1;
        }
        let txt = std::str::from_utf8(&self.s[start..self.pos]).unwrap();
        let n: f64 = txt.parse().expect("fixtures JSON: bad number");
        JsonValue::Number(n)
    }
    fn parse_bool(&mut self) -> JsonValue {
        if self.s[self.pos..].starts_with(b"true") {
            self.pos += 4;
            JsonValue::Bool(true)
        } else if self.s[self.pos..].starts_with(b"false") {
            self.pos += 5;
            JsonValue::Bool(false)
        } else {
            panic!("fixtures JSON: bad bool at {}", self.pos);
        }
    }
    fn parse_null(&mut self) -> JsonValue {
        if self.s[self.pos..].starts_with(b"null") {
            self.pos += 4;
            JsonValue::Null
        } else {
            panic!("fixtures JSON: bad null at {}", self.pos);
        }
    }
}

impl JsonValue {
    fn as_object(&self) -> &[(String, JsonValue)] {
        match self {
            JsonValue::Object(v) => v,
            _ => panic!("fixtures JSON: expected object"),
        }
    }
    fn as_array(&self) -> &[JsonValue] {
        match self {
            JsonValue::Array(v) => v,
            _ => panic!("fixtures JSON: expected array"),
        }
    }
    fn as_int(&self) -> i64 {
        match self {
            JsonValue::Number(n) => *n as i64,
            _ => panic!("fixtures JSON: expected number"),
        }
    }
    fn get<'a>(&'a self, key: &str) -> &'a JsonValue {
        match self {
            JsonValue::Object(v) => v
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .unwrap_or_else(|| panic!("fixtures JSON: missing key {key}")),
            _ => panic!("fixtures JSON: expected object"),
        }
    }
}

fn parse_fixtures_json(v: &JsonValue) -> Vec<(String, Vec<(i16, i16)>)> {
    let mut out = Vec::new();
    for (name, body) in v.as_object() {
        let moves_v = body.get("moves").as_array();
        let mut moves = Vec::with_capacity(moves_v.len());
        for m in moves_v {
            let pair = m.as_array();
            if pair.len() != 2 {
                panic!("fixture {name}: move must be [q, r]");
            }
            let q = i16::try_from(pair[0].as_int())
                .unwrap_or_else(|_| panic!("fixture {name}: q out of i16"));
            let r = i16::try_from(pair[1].as_int())
                .unwrap_or_else(|_| panic!("fixture {name}: r out of i16"));
            moves.push((q, r));
        }
        out.push((name.clone(), moves));
    }
    out
}

fn is_snake_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !s.starts_with(|c: char| c.is_ascii_digit())
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
    // Phase 16: Layer 2 S1/S2 ablation compile-time default.
    emit_bool(
        out,
        cfg,
        &["engine", "eval", "eval_s1s2_default"],
        "EVAL_S1S2_DEFAULT",
    );
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
    emit_usize(
        out,
        cfg,
        &["engine", "threats", "max_incremental_centers"],
        "MAX_INCREMENTAL_CENTERS",
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

#[allow(clippy::too_many_lines)]
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
    emit_f32(
        out,
        cfg,
        &["engine", "search", "time_stone1_pct"],
        "TIME_STONE1_PCT",
    );
    emit_i32(
        out,
        cfg,
        &["engine", "search", "asp_window_initial"],
        "ASP_WINDOW_INITIAL",
    );
    emit_u32(
        out,
        cfg,
        &["engine", "search", "asp_window_widen_factor"],
        "ASP_WINDOW_WIDEN_FACTOR",
    );
    emit_i8(
        out,
        cfg,
        &["engine", "search", "lmr_min_depth"],
        "LMR_MIN_DEPTH",
    );
    emit_u8(
        out,
        cfg,
        &["engine", "search", "lmr_min_move_index"],
        "LMR_MIN_MOVE_INDEX",
    );
    emit_i8(
        out,
        cfg,
        &["engine", "search", "lmr_reduction"],
        "LMR_REDUCTION",
    );
    emit_u8(
        out,
        cfg,
        &["engine", "search", "qsearch_max_plies"],
        "QSEARCH_MAX_PLIES",
    );
    emit_u8(
        out,
        cfg,
        &["engine", "search", "max_check_extensions"],
        "MAX_CHECK_EXTENSIONS",
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

fn emit_u8(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = as_int(get(cfg, path), path);
    writeln!(out, "pub const {name}: u8 = {v};").unwrap();
}

fn emit_bool(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = get(cfg, path)
        .as_bool()
        .unwrap_or_else(|| panic!("hexo.toml {} not a bool", path.join(".")));
    writeln!(out, "pub const {name}: bool = {v};").unwrap();
}

fn emit_f32(out: &mut String, cfg: &toml::Value, path: &[&str], name: &str) {
    let v = get(cfg, path)
        .as_float()
        .unwrap_or_else(|| panic!("hexo.toml {} not a float", path.join(".")));
    writeln!(out, "pub const {name}: f32 = {v}_f32;").unwrap();
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
