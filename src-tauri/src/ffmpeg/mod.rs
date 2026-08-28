//! Small, version-pinned FFmpeg API surface shared by casting and future media tooling.
//!
//! The application ships FFmpeg 8 alongside libmpv.  Keeping the bindings intentionally small
//! avoids a second FFmpeg build while making direct demux/mux and future frame extraction use the
//! same runtime as playback.

pub(crate) mod ffi;
pub(crate) mod remux;
