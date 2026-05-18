use crate::board::Board;
use crate::search::SearchConfig;
use crate::tt::TranspositionTable;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

#[pyclass]
pub struct Engine {
    _board: Board,
    _search: SearchConfig,
    _tt: TranspositionTable,
}

#[pymethods]
impl Engine {
    #[new]
    #[pyo3(signature = (tt_size_mb = None))]
    fn new(tt_size_mb: Option<usize>) -> Self {
        let tt_mb = tt_size_mb.unwrap_or(crate::config::DEFAULT_TT_SIZE_MB);
        Self {
            _board: Board::new(),
            _search: SearchConfig::default(),
            _tt: TranspositionTable::new(tt_mb),
        }
    }

    fn place(&mut self, _pos: (i16, i16)) -> PyResult<()> {
        Err(PyValueError::new_err("not implemented"))
    }

    fn undo(&mut self) -> PyResult<()> {
        Err(PyValueError::new_err("not implemented"))
    }

    #[pyo3(signature = (time_ms = None, depth = None))]
    fn best_move(&mut self, time_ms: Option<u64>, depth: Option<i8>) -> PyResult<(i16, i16)> {
        let _ = (time_ms, depth);
        Err(PyValueError::new_err("not implemented"))
    }

    fn eval(&self) -> i32 {
        0
    }

    fn state<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        Ok(PyDict::new(py))
    }

    fn reset(&mut self) {
        self._board = Board::new();
    }

    fn load_bsn(&mut self, _s: &str) -> PyResult<()> {
        Err(PyValueError::new_err("not implemented"))
    }

    fn dump_bsn(&self) -> String {
        String::new()
    }

    fn to_move(&self) -> u8 {
        0
    }

    fn is_legal(&self, _pos: (i16, i16)) -> bool {
        false
    }

    fn winner(&self) -> Option<u8> {
        None
    }

    fn ply(&self) -> u32 {
        0
    }
}

#[pymodule]
fn hexo_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Engine>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
