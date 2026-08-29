//! Entrypoints for `InvoiceContract`, split by concern. Each submodule adds its
//! own `#[contractimpl] impl InvoiceContract { ... }` block; soroban-sdk merges
//! them into a single generated client, so this split changes no public ABI.

mod admin;
mod batch;
mod lifecycle;
