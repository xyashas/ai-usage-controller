//! AI Usage Controller core engine.
//!
//! Everything runs locally. Parsers read the session logs that Claude Code,
//! Codex CLI and Gemini CLI already write to disk, turn them into
//! [`model::UsageEvent`]s, and the [`engine::Engine`] turns those into a
//! [`snapshot::Snapshot`] the widget renders.
//!
//! Privacy rule: no prompt text, code or file contents are ever stored.
//! Parsers keep token counts, model names, timestamps and a short
//! tool-call signature (tool name + file path) used only for loop detection,
//! which never leaves the machine.

pub mod advice;
pub mod engine;
pub mod handoff;
pub mod entitlements;
pub mod licence;
pub mod live;
pub mod metrics;
pub mod model;
pub mod parsers;
pub mod paths;
pub mod plans;
pub mod pricing;
pub mod runaway;
pub mod scanner;
pub mod settings;
pub mod snapshot;
pub mod stop;
pub mod store;
pub mod sync;

pub use engine::Engine;
pub use model::{Tool, UsageEvent};
pub use snapshot::Snapshot;
