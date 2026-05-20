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
//! Shared utilities for criterion micro-benches.

pub mod positions;
