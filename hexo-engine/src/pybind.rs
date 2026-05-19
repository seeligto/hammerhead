//! `PyO3` wrapper. Thin shim over [`crate::search::Engine`]; no game logic.
//!
//! All search work runs inside `py.detach`, so long-running `best_move`
//! calls release the GIL.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::board::Player;
use crate::coords::Coord;
use crate::search::Engine as RustEngine;

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
}

#[pymodule]
fn hexo_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyEngine>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
