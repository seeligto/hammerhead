//! `PyO3` wrapper. Thin shim over [`crate::engine::Engine`]; no game logic.
//!
//! All search work runs inside `py.detach`, so long-running `best_move`
//! calls release the GIL.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::board::Player;
use crate::coords::Coord;
use crate::engine::Engine as RustEngine;
use crate::eval_overrides::EvalOverrides;
use crate::search::SearchConfig;

/// `Board` keeps a few `RefCell` / `Cell` caches (lazy threat sets, lazy
/// static eval), so the wrapper is `!Sync`. `unsendable` lifts `PyO3`'s
/// `Send + Sync` requirement; we still get `Send` automatically because
/// every field is `Send`, which is enough for `Python::detach` (its
/// `Ungil` bound is `T: Send`).
#[pyclass(name = "Engine", unsendable)]
pub struct PyEngine {
    inner: RustEngine,
}

#[pymethods]
impl PyEngine {
    #[new]
    #[pyo3(signature = (tt_size_mb = None))]
    fn new(tt_size_mb: Option<usize>) -> Self {
        let mb = tt_size_mb.unwrap_or(crate::config::DEFAULT_TT_SIZE_MB);
        Self {
            inner: RustEngine::new(mb),
        }
    }

    fn place(&mut self, pos: (i16, i16)) -> PyResult<()> {
        let c = Coord::new(pos.0, pos.1);
        self.inner
            .place(c)
            .map_err(|e| PyValueError::new_err(format!("place failed: {e}")))
    }

    fn undo(&mut self) -> PyResult<()> {
        self.inner
            .undo()
            .map_err(|e| PyValueError::new_err(format!("undo failed: {e}")))
    }

    #[pyo3(signature = (time_ms = None, depth = None))]
    fn best_move(
        &mut self,
        py: Python<'_>,
        time_ms: Option<u64>,
        depth: Option<i8>,
    ) -> PyResult<(i16, i16)> {
        if time_ms.is_none() && depth.is_none() {
            return Err(PyValueError::new_err(
                "best_move requires time_ms or depth",
            ));
        }
        let result = py.detach(|| self.inner.best_move(time_ms, depth));
        Ok((result.best_move.q, result.best_move.r))
    }

    /// Bench-only variant returning the full search result as
    /// `(q, r, score, depth_reached, nodes, time_ms)`. Lets the Python
    /// macro-bench library compute NPS and depth-at-time without going
    /// through `cargo bench`.
    #[pyo3(signature = (time_ms = None, depth = None))]
    fn bench_best_move(
        &mut self,
        py: Python<'_>,
        time_ms: Option<u64>,
        depth: Option<i8>,
    ) -> PyResult<(i16, i16, i32, i8, u64, u64)> {
        if time_ms.is_none() && depth.is_none() {
            return Err(PyValueError::new_err(
                "bench_best_move requires time_ms or depth",
            ));
        }
        let r = py.detach(|| self.inner.best_move(time_ms, depth));
        Ok((
            r.best_move.q,
            r.best_move.r,
            r.score,
            r.depth_reached,
            r.nodes,
            r.time_ms,
        ))
    }

    fn find_pv(&mut self, depth: i8) -> Vec<(i16, i16)> {
        self.inner
            .find_pv(depth)
            .into_iter()
            .map(|c| (c.q, c.r))
            .collect()
    }

    fn cached_eval(&self) -> i32 {
        self.inner.cached_eval()
    }

    fn to_move(&self) -> u8 {
        match self.inner.to_move() {
            Player::X => 0,
            Player::O => 1,
        }
    }

    fn winner(&self) -> Option<u8> {
        self.inner.winner().map(|p| match p {
            Player::X => 0,
            Player::O => 1,
        })
    }

    fn ply(&self) -> u32 {
        self.inner.ply()
    }

    fn halfmove(&self) -> u8 {
        self.inner.halfmove()
    }

    fn hash(&self) -> u128 {
        self.inner.hash()
    }

    fn reset(&mut self) {
        self.inner.reset();
    }

    fn clear_tt(&mut self) {
        self.inner.clear_tt();
    }

    /// TT diagnostics snapshot as a Python dict. Keys:
    /// `n_slots`, `occupied`, `generation`, `probes`, `hits`,
    /// `stores`, `collisions`. The four counter fields are populated
    /// only when the engine was built with Cargo feature `tt_stats`;
    /// otherwise they read as `0`. Callers can branch on
    /// `dict["probes"] == 0` to detect "no stats available".
    fn tt_stats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let s = self.inner.tt_stats();
        let d = PyDict::new(py);
        d.set_item("n_slots", s.n_slots)?;
        d.set_item("occupied", s.occupied)?;
        d.set_item("generation", s.generation)?;
        d.set_item("probes", s.probes)?;
        d.set_item("hits", s.hits)?;
        d.set_item("stores", s.stores)?;
        d.set_item("collisions", s.collisions)?;
        Ok(d)
    }

    /// Snapshot of the currently-active runtime eval overrides as a
    /// Python dict. Keys mirror the `EvalOverrides` field names.
    /// Defaults equal `crate::config::*` (codegen'd from hexo.toml).
    fn eval_overrides<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let ov = self.inner.eval_overrides();
        let d = PyDict::new(py);
        d.set_item("open_5", ov.open_5)?;
        d.set_item("closed_5", ov.closed_5)?;
        d.set_item("open_4", ov.open_4)?;
        d.set_item("closed_4", ov.closed_4)?;
        d.set_item("open_3", ov.open_3)?;
        d.set_item("closed_3", ov.closed_3)?;
        d.set_item("open_2", ov.open_2)?;
        d.set_item("rhombus", ov.rhombus)?;
        d.set_item("rhombus_isolation_radius", ov.rhombus_isolation_radius)?;
        d.set_item("window_k_scores", ov.window_k_scores.to_vec())?;
        d.set_item("open_extension_factor", ov.open_extension_factor)?;
        d.set_item("closed_extension_factor", ov.closed_extension_factor)?;
        d.set_item("fork_cover2_bonus", ov.fork_cover2_bonus)?;
        Ok(d)
    }

    /// Patch the runtime eval overrides. Partial updates allowed:
    /// missing keys retain their *current* value (not defaults — the
    /// call is incremental). Unknown keys raise `ValueError`.
    ///
    /// Recognised keys (match `EvalOverrides` fields exactly):
    /// `open_5`, `closed_5`, `open_4`, `closed_4`,
    /// `open_3`, `closed_3`, `open_2`,
    /// `rhombus`, `rhombus_isolation_radius`,
    /// `window_k_scores` (sequence of 7 ints, including index 6 ==
    /// mate score), `open_extension_factor`,
    /// `closed_extension_factor`, `fork_cover2_bonus`.
    ///
    /// Persists across `reset()` (Phase 18 precedent).
    fn set_eval_overrides(&mut self, overrides: &Bound<'_, PyDict>) -> PyResult<()> {
        let next = build_overrides_from_dict(self.inner.eval_overrides(), overrides)?;
        self.inner.set_eval_overrides(next);
        Ok(())
    }

    /// Snapshot of the currently-active search params (Sprint 4A).
    /// Keys mirror the public fields of [`SearchConfig`] that are
    /// safe to expose for tuning. Defaults equal `crate::config::*`
    /// (codegen'd from hexo.toml) until `set_search_params` runs.
    fn search_params<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let cfg = &self.inner.cfg;
        let d = PyDict::new(py);
        d.set_item("lmr_min_depth", cfg.lmr_min_depth)?;
        d.set_item("lmr_min_move_index", cfg.lmr_min_move_index)?;
        d.set_item("lmr_reduction", cfg.lmr_reduction)?;
        d.set_item("asp_window_initial", cfg.asp_window_initial)?;
        d.set_item("asp_window_widen_factor", cfg.asp_window_widen_factor)?;
        d.set_item("max_check_extensions", cfg.max_check_extensions)?;
        d.set_item("qsearch_max_plies", cfg.qsearch_max_plies)?;
        d.set_item("move_gen_cap", cfg.move_gen_cap)?;
        Ok(d)
    }

    /// Patch the runtime search params (Sprint 4A + 4C). Partial updates:
    /// missing keys retain their *current* value (incremental, not
    /// reset-then-set). Unknown keys raise `ValueError`.
    ///
    /// Recognised keys:
    /// - Sprint 4A — LMR: `lmr_min_depth` (i8, [1, 32]),
    ///   `lmr_min_move_index` (u8, no upper bound),
    ///   `lmr_reduction` (i8, [0, 4]).
    /// - Sprint 4C — aspiration + extension:
    ///   `asp_window_initial` (i32, [1, 10000]),
    ///   `asp_window_widen_factor` (u32, [2, 16]),
    ///   `max_check_extensions` (u8, [0, 32]),
    ///   `qsearch_max_plies` (u8, [0, 32]).
    /// - Cap probe — candidate-set size:
    ///   `move_gen_cap` (usize, [1, 256]).
    ///
    /// Persists across `reset()` and `clear_tt()`; does NOT survive
    /// engine restart. Use `reset_search_params` to restore defaults.
    fn set_search_params(&mut self, params: &Bound<'_, PyDict>) -> PyResult<()> {
        let next = build_search_params_from_dict(self.inner.cfg, params)?;
        self.inner.cfg = next;
        Ok(())
    }

    /// Reset search params to `SearchConfig::default()` (TOML-sourced).
    fn reset_search_params(&mut self) {
        self.inner.cfg = SearchConfig::default();
    }
}

/// Merge `dict` into `current`. Unknown keys are a `ValueError` (catches
/// typos before they silently no-op). `window_k_scores` accepts any
/// 7-element iterable (list or tuple of ints).
fn build_overrides_from_dict(
    current: EvalOverrides,
    dict: &Bound<'_, PyDict>,
) -> PyResult<EvalOverrides> {
    let mut next = current;
    for (k, v) in dict.iter() {
        let key: String = k.extract()?;
        match key.as_str() {
            "open_5" => next.open_5 = v.extract()?,
            "closed_5" => next.closed_5 = v.extract()?,
            "open_4" => next.open_4 = v.extract()?,
            "closed_4" => next.closed_4 = v.extract()?,
            "open_3" => next.open_3 = v.extract()?,
            "closed_3" => next.closed_3 = v.extract()?,
            "open_2" => next.open_2 = v.extract()?,
            "rhombus" => next.rhombus = v.extract()?,
            "rhombus_isolation_radius" => next.rhombus_isolation_radius = v.extract()?,
            "open_extension_factor" => next.open_extension_factor = v.extract()?,
            "closed_extension_factor" => next.closed_extension_factor = v.extract()?,
            "fork_cover2_bonus" => next.fork_cover2_bonus = v.extract()?,
            "window_k_scores" => {
                // Accept any sequence; `extract::<Vec<i32>>` covers
                // lists, tuples, and other iterables. PyO3 0.28
                // deprecated the explicit `downcast::<PyList>`
                // / `<PyTuple>` paths in favour of this trait route.
                let vals: Vec<i32> = v.extract()?;
                if vals.len() != 7 {
                    return Err(PyValueError::new_err(format!(
                        "window_k_scores must have 7 entries, got {}",
                        vals.len()
                    )));
                }
                let mut arr = [0i32; 7];
                arr.copy_from_slice(&vals);
                next.window_k_scores = arr;
            }
            _ => {
                return Err(PyValueError::new_err(format!(
                    "unknown eval override key: {key:?}"
                )));
            }
        }
    }
    Ok(next)
}

/// Merge `dict` into `current`. LMR triplet (Sprint 4A) plus
/// aspiration + extension knobs (Sprint 4C).
fn build_search_params_from_dict(
    current: SearchConfig,
    dict: &Bound<'_, PyDict>,
) -> PyResult<SearchConfig> {
    let mut next = current;
    for (k, v) in dict.iter() {
        let key: String = k.extract()?;
        match key.as_str() {
            "lmr_min_depth" => {
                let d: i8 = v.extract()?;
                if !(1..=32).contains(&d) {
                    return Err(PyValueError::new_err(format!(
                        "lmr_min_depth out of range [1, 32]: {d}"
                    )));
                }
                next.lmr_min_depth = d;
            }
            "lmr_min_move_index" => {
                next.lmr_min_move_index = v.extract()?;
            }
            "lmr_reduction" => {
                let r: i8 = v.extract()?;
                if !(0..=4).contains(&r) {
                    return Err(PyValueError::new_err(format!(
                        "lmr_reduction out of range [0, 4]: {r}"
                    )));
                }
                next.lmr_reduction = r;
            }
            "asp_window_initial" => {
                let w: i32 = v.extract()?;
                if !(1..=10_000).contains(&w) {
                    return Err(PyValueError::new_err(format!(
                        "asp_window_initial out of range [1, 10000]: {w}"
                    )));
                }
                next.asp_window_initial = w;
            }
            "asp_window_widen_factor" => {
                let f: u32 = v.extract()?;
                if !(2..=16).contains(&f) {
                    return Err(PyValueError::new_err(format!(
                        "asp_window_widen_factor out of range [2, 16]: {f}"
                    )));
                }
                next.asp_window_widen_factor = f;
            }
            "max_check_extensions" => {
                let e: u8 = v.extract()?;
                if e > 32 {
                    return Err(PyValueError::new_err(format!(
                        "max_check_extensions out of range [0, 32]: {e}"
                    )));
                }
                next.max_check_extensions = e;
            }
            "qsearch_max_plies" => {
                let p: u8 = v.extract()?;
                if p > 32 {
                    return Err(PyValueError::new_err(format!(
                        "qsearch_max_plies out of range [0, 32]: {p}"
                    )));
                }
                next.qsearch_max_plies = p;
            }
            "move_gen_cap" => {
                let c: usize = v.extract()?;
                if !(1..=256).contains(&c) {
                    return Err(PyValueError::new_err(format!(
                        "move_gen_cap out of range [1, 256]: {c}"
                    )));
                }
                next.move_gen_cap = c;
            }
            _ => {
                return Err(PyValueError::new_err(format!(
                    "unknown search param key: {key:?}"
                )));
            }
        }
    }
    Ok(next)
}

#[pymodule]
fn hammerhead_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyEngine>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
