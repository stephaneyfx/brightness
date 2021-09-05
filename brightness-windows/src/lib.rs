// Copyright (C) 2021 The brightness authors. Distributed under the 0BSD license.

//! # Overview
//! - [📦 crates.io](https://crates.io/crates/brightness-windows)
//! - [📖 Documentation](https://docs.rs/brightness-windows)
//! - [⚖ 0BSD license](https://spdx.org/licenses/0BSD.html)
//!
//! This crate generates Windows bindings to get and set display brightness.
//!
//! See crate [brightness](https://docs.rs/brightness) for details.
//!
//! # Contribute
//!
//! All contributions shall be licensed under the [0BSD license](https://spdx.org/licenses/0BSD.html).

#![cfg(windows)]
#![deny(warnings)]

windows::include_bindings!();
