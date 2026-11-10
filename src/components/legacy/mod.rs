//! The first physics model, `PhysicsComponent` and `RigidbodyComponent`
//! (moved from the engine's `pulsar_physics`, Pulsar-Native#1605). Levels
//! still carry them, and a `PhysicsComponent` on the `WaterSim` channel
//! pushes simulated water (its `WaterHitboxSourceRow`). The simulation reads
//! the current components (`super`); these go once `level_migrate` moves
//! levels to them.

mod physics_component;
mod rigidbody_component;

pub use physics_component::*;
pub use rigidbody_component::*;
