//! One engine owns local/remote selection and the selected mode's presentation worker.

pub(crate) mod backend;
pub(crate) mod engine;
pub(crate) mod local;
pub(crate) mod mode;
pub(crate) mod models;
pub(crate) mod remote;
pub(crate) mod runtime;
pub(crate) mod server;
pub(crate) mod start;

pub(crate) mod server_target;
pub(crate) mod queue_controller;
pub(crate) mod queue_projection;
