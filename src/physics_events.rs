//! The `physics` event namespace.
//!
//! Every physics component can raise events that scripts and Rust
//! subscribers hook like any other engine event, under Gamma's `physics`
//! namespace: `physics::ColliderComponent.collision_enter`,
//! `physics::RigidBodyComponent.sleep`, and so on. They are published on
//! the component owner's entity channel (the same channel component
//! callbacks and `#[bp_handler]`s use), so a script attached to the object
//! hears its own components' events.
//!
//! Each event carries three entities, in this order:
//!
//! * `entity`: the object the component is on,
//! * `component`: the component instance that raised it,
//! * `other`: the other party (the thing hit, entered, or connected to),
//!   or `0` when there is none.
//!
//! A component declares its events in its own file with [`declare_events!`],
//! which names them for the behavior code and lists them for the simulation
//! (`register_all`). The crate's registrations (`crate::registrations`) put
//! each in the script editor's event list (a
//! `pulsar_world_registry::ComponentEventRegistration`). The namespace rides
//! in the event name (`physics::...`), which the script bridge and the hub
//! both understand.

use std::collections::HashMap;

use pulsar_events::gamma::{Channel, DynEvent, DynValue, EventDescriptor, FieldType};
use pulsar_events::{EventCategory, EventHub};
use pulsar_world_registry::pulsar_script_vm::{EventDecl, EventField, Type};

use crate::behavior::PhysEvent;

/// The Gamma namespace of every event in this module.
pub const NAMESPACE: &str = "physics";

/// The fields every physics event has.
pub const FIELDS: [&str; 3] = ["entity", "component", "other"];

/// One declared event: the component class it belongs to and its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EventSpec {
    pub class: &'static str,
    pub name: &'static str,
}

inventory::collect!(EventSpec);

impl EventSpec {
    /// The event's name inside its namespace: `Class.event`.
    pub fn local_name(&self) -> String {
        format!("{}.{}", self.class, self.name)
    }

    /// The name scripts use: `physics::Class.event`.
    pub fn qualified_name(&self) -> String {
        format!("{NAMESPACE}::{}", self.local_name())
    }

    /// The Gamma descriptor.
    pub fn descriptor(&self) -> EventDescriptor {
        EventDescriptor::dynamic_in(
            NAMESPACE,
            self.local_name(),
            FIELDS.map(|field| (field, FieldType::U64)),
        )
        .expect("`physics` is not a reserved namespace")
    }

    /// The script-side declaration.
    pub fn declaration(&self) -> EventDecl {
        EventDecl {
            name: self.qualified_name(),
            fields: FIELDS
                .map(|field| EventField::new(field, Type::Entity))
                .to_vec(),
        }
    }
}

/// Declare physics events in a component's file:
///
/// ```ignore
/// declare_events! {
///     pub const ENTER = (COLLIDER_CLASS_NAME, "collision_enter");
/// }
/// ```
///
/// Each becomes an [`EventSpec`] constant and is registered on every hub the
/// simulation runs against; `crate::registrations` offers it to scripts.
#[macro_export]
macro_rules! declare_events {
    ($($(#[$meta:meta])* $vis:vis const $ident:ident = ($class:expr, $name:literal);)*) => {$(
        $(#[$meta])*
        $vis const $ident: $crate::physics_events::EventSpec =
            $crate::physics_events::EventSpec { class: $class, name: $name };
        ::inventory::submit! { $ident }
    )*};
}

/// The hub ids of the declared events.
pub type EventIds = HashMap<EventSpec, u64>;

/// Register every declared event on `hub` (idempotent), so subscribers can
/// find them before the first one fires, and return their ids.
pub fn register_all(hub: &EventHub) -> EventIds {
    let mut ids = EventIds::new();
    for spec in inventory::iter::<EventSpec> {
        match hub.register(spec.descriptor(), EventCategory::Physics) {
            Ok(id) => {
                ids.insert(*spec, id);
            }
            Err(error) => {
                tracing::warn!(event = %spec.qualified_name(), %error, "physics event could not be registered");
            }
        }
    }
    ids
}

/// Queue `event` on its object's entity channel.
pub fn publish(hub: &EventHub, ids: &EventIds, event: &PhysEvent) {
    let Some(&id) = ids.get(&event.spec) else {
        return;
    };
    let fields = vec![
        DynValue::U64(event.owner.bits()),
        DynValue::U64(event.component.bits()),
        DynValue::U64(event.other.map_or(0, |e| e.bits())),
    ];
    if let Err(error) = hub.publish_dyn(
        Channel::Entity(event.owner.bits()),
        DynEvent::new(id, fields),
    ) {
        tracing::warn!(event = %event.spec.qualified_name(), %error, "physics event was refused");
    }
}
