use engine_class_derive::world_component;

use super::PhysicsComponent;

// A World-resident typed component (Pulsar-Native#556), registered by the
// plugin's root (`crate::registrations`). The simulation does not read it:
// it is the first physics model, kept for the levels that carry it (its
// water hitbox included) until `level_migrate` moves them to the current
// components.
#[world_component]
impl PhysicsComponent {}
