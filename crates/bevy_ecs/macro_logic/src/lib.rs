//! Reusable `bevy_ecs` macro logic. This enables defining derives that internally derive ECS traits
//! like `Component`.

/// `Component` macro logic. The primary interface is [`DeriveComponent`](component::DeriveComponent).
pub mod component;

/// `bsn!` parsing and code generation, shared by `bevy_scene`'s `bsn!` macro and `#[require]`.
#[doc(hidden)]
pub mod bsn;

/// `MapEntities` macro logic.
pub mod map_entities;
