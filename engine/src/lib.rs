// SPDX-FileCopyrightText: 2026 Eshan05
// SPDX-License-Identifier: GPL-3.0-or-later

//! Read-only authored release-note retrieval. No inventory scanning or package updates.
pub const APP_NAME: &str = "Chronver";
pub const USER_AGENT: &str = concat!("Chronver/", env!("CARGO_PKG_VERSION"));

pub mod catalog;
pub mod batch;
pub mod dependencies;
pub mod engine;
pub mod extensions;
mod github_dates;
pub mod http;
mod http_cache;
pub mod icons;
pub mod inventory;
pub mod inventory_refresh;
pub mod matching;
pub mod model;
pub mod parse;
pub mod presentation;
mod publication_dates;
pub mod source_discovery;
pub mod publisher_metadata;
pub mod version;
pub mod watchlist;

pub use engine::Engine;
pub use model::*;
