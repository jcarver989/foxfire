//! Runs Foxfire Cloud workspaces on a local Docker engine.
//!
//! A [`project::Project`] implements the parts of the desktop's command-driven
//! remotes contract that need more than one Docker command. Each instance is a
//! container started from the project's warm image and moved to freshly
//! fetched upstream source. Stop exports its work to a Git ref in the host
//! checkout before removing it.
//!
//! Git objects travel in both directions over `docker exec`, using Git's own
//! `ext::` transport, so nothing from the host is mounted into a container.

pub mod cli;
pub mod error;
mod process;
pub mod project;
