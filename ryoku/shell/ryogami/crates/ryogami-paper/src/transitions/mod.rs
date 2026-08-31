//! The unified transition surface: ryoku's reveal stage ([`reveal`]) plus the
//! skwd catalog (`crate::transition_paper`). The daemon's picker chooses a
//! [`reveal::Transition`]; the renderer runs it on the shared GL context.

pub mod reveal;

pub use reveal::{RevealProgram, Transition, cubic_bezier_ease, kind_code};
