//! Core primitives: tasks, claims, agents, messages, kv.
//!
//! These are the local-loop extensions on top of the A2A subset that
//! make agentd useful as a dev-loop coordinator.

pub mod action_runner;
pub mod actions;
pub mod emote;
pub mod emote_v2;
pub mod store;
pub mod types;
pub mod validator;
pub mod windows_security;
