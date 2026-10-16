//! Data shared by the Aura compiler and the native runtime.
//!
//! This crate holds semantic types, diagnostics, integer helpers, and the
//! operator enums that both sides of the native boundary must agree on. It
//! contains no parser, checker, or code generator.

pub mod diag;
pub mod integer;
pub mod ops;
pub mod task;
pub mod type_budget;
pub mod types;
