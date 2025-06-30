//! Centralized storage for observers, allowing for efficient look-ups.
//!
//! This has multiple levels:
//! - [`World::observers`] provides access to [`Observers`], which is a central storage for all observers.
//! - [`Observers`] contains multiple distinct caches in the form of [`CachedObservers`].
//!     - Most observers are looked up by the [`ComponentId`] of the event they are observing
//!     - Lifecycle observers have their own fields to save lookups.
//! - [`CachedObservers`] contains maps of [`ObserverRunner`]s, which are the actual functions that will be run when the observer is triggered.
//!     - These are split by target type, in order to allow for different lookup strategies.
//!     - [`CachedComponentObservers`] is one of these maps, which contains observers that are specifically targeted at a component.

use bevy_platform::collections::HashMap;

use crate::{
    archetype::ArchetypeFlags,
    change_detection::MaybeLocation,
    component::ComponentId,
    entity::EntityHashMap,
    lifecycle::ADD,
    observer::{ObserverDescriptor, ObserverRunner, ObserverTrigger},
    prelude::*,
    world::DeferredWorld,
};

/// An internal lookup table tracking all of the observers in the world.
///
/// Stores a cache mapping trigger ids to the registered observers.
/// Some observer kinds (like [lifecycle](crate::lifecycle) observers) have a dedicated field,
/// saving lookups for the most common triggers.
///
/// This can be accessed via [`World::observers`].
#[derive(Default, Debug)]
pub struct Observers {
    // Cached ECS observers to save a lookup most common triggers.
    add: CachedObservers,
    insert: CachedObservers,
    replace: CachedObservers,
    remove: CachedObservers,
    despawn: CachedObservers,
    // Map from trigger type to set of observers listening to that trigger
    cache: HashMap<ComponentId, CachedObservers>,
}

impl Observers {
    /// Registers a new observer.
    ///
    /// This is driven by hooks on the [`ObserverDescriptor`] component,
    /// which serves as the source of truth for observers.
    pub(super) fn register_observer(
        &mut self,
        observer_entity: Entity,
        observer_runner: ObserverRunner,
        descriptor: &ObserverDescriptor,
    ) {
        for &event_id in descriptor.events() {
            // Special case the lifecycle events, which have their own fields for perf
            if event_id == ADD {
                self.add
                    .register_observer(observer_entity, observer_runner, descriptor);
            } else if event_id == crate::lifecycle::INSERT {
                self.insert
                    .register_observer(observer_entity, observer_runner, descriptor);
            } else if event_id == crate::lifecycle::REPLACE {
                self.replace
                    .register_observer(observer_entity, observer_runner, descriptor);
            } else if event_id == crate::lifecycle::REMOVE {
                self.remove
                    .register_observer(observer_entity, observer_runner, descriptor);
            } else if event_id == crate::lifecycle::DESPAWN {
                self.despawn
                    .register_observer(observer_entity, observer_runner, descriptor);
            } else {
                // For all other events, use the cache
                self.cache.entry(event_id).or_default().register_observer(
                    observer_entity,
                    observer_runner,
                    descriptor,
                );
            }
        }
    }

    /// Unregisters an observer.
    ///
    /// This is driven by hooks on the [`ObserverDescriptor`] component,
    /// which serves as the source of truth for observers.
    ///
    /// This method will fail silently if the observer is not found.
    pub(super) fn unregister_observer(
        &mut self,
        observer_entity: Entity,
        descriptor: &ObserverDescriptor,
    ) {
        for &event_id in descriptor.events() {
            // Special case the lifecycle events, which have their own fields for perf
            if event_id == ADD {
                self.add.unregister_observer(observer_entity, descriptor);
            } else if event_id == crate::lifecycle::INSERT {
                self.insert.unregister_observer(observer_entity, descriptor);
            } else if event_id == crate::lifecycle::REPLACE {
                self.replace
                    .unregister_observer(observer_entity, descriptor);
            } else if event_id == crate::lifecycle::REMOVE {
                self.remove.unregister_observer(observer_entity, descriptor);
            } else if event_id == crate::lifecycle::DESPAWN {
                self.despawn
                    .unregister_observer(observer_entity, descriptor);
            } else {
                // For all other events, use the cache
                self.cache
                    .entry(event_id)
                    .or_default()
                    .unregister_observer(observer_entity, descriptor);
            }
        }
    }

    /// Attempts to get the observers for the given `event_type`.
    ///
    /// When accessing the observers for lifecycle events, such as [`Add`], [`Insert`], [`Replace`], [`Remove`], and [`Despawn`],
    /// use the [`ComponentId`] constants from the [`lifecycle`](crate::lifecycle) module.
    pub fn try_get_observers(&self, event_type: ComponentId) -> Option<&CachedObservers> {
        use crate::lifecycle::*;

        match event_type {
            ADD => Some(&self.add),
            INSERT => Some(&self.insert),
            REPLACE => Some(&self.replace),
            REMOVE => Some(&self.remove),
            DESPAWN => Some(&self.despawn),
            _ => self.cache.get(&event_type),
        }
    }

    /// This will run the observers of the given `event_type`, targeting the given `entity` and `components`.
    pub(crate) fn invoke<T>(
        mut world: DeferredWorld,
        event_type: ComponentId,
        current_target: Option<Entity>,
        original_target: Option<Entity>,
        components: impl Iterator<Item = ComponentId> + Clone,
        data: &mut T,
        propagate: &mut bool,
        caller: MaybeLocation,
    ) {
        // SAFETY: You cannot get a mutable reference to `observers` from `DeferredWorld`
        let (mut world, observers) = unsafe {
            let world = world.as_unsafe_world_cell();
            // SAFETY: There are no outstanding world references
            world.increment_trigger_id();
            let observers = world.observers();
            let Some(observers) = observers.try_get_observers(event_type) else {
                return;
            };
            // SAFETY: The only outstanding reference to world is `observers`
            (world.into_deferred(), observers)
        };

        let trigger_for_components = components.clone();

        let mut trigger_observer = |(&observer, runner): (&Entity, &ObserverRunner)| {
            (runner)(
                world.reborrow(),
                ObserverTrigger {
                    observer,
                    event_type,
                    components: components.clone().collect(),
                    current_target,
                    original_target,
                    caller,
                },
                data.into(),
                propagate,
            );
        };
        // Trigger observers listening for any kind of this trigger
        observers
            .universal_observers
            .iter()
            .for_each(&mut trigger_observer);

        // Trigger entity observers listening for this kind of trigger
        if let Some(target_entity) = current_target {
            if let Some(map) = observers.entity_observers.get(&target_entity) {
                map.iter().for_each(&mut trigger_observer);
            }
        }

        // Trigger observers listening to this trigger targeting a specific component
        trigger_for_components.for_each(|id| {
            if let Some(component_observers) = observers.component_observers.get(&id) {
                component_observers
                    .universal_observers
                    .iter()
                    .for_each(&mut trigger_observer);

                if let Some(target_entity) = current_target {
                    if let Some(map) = component_observers
                        .entity_component_observers
                        .get(&target_entity)
                    {
                        map.iter().for_each(&mut trigger_observer);
                    }
                }
            }
        });
    }

    /// Updates the [`ArchetypeFlags`] based on the presence of observers watching the given `component_id`.
    // TODO: these flags are never unset, even if the observer is removed.
    pub(crate) fn update_archetype_flags(
        &self,
        component_id: ComponentId,
        flags: &mut ArchetypeFlags,
    ) {
        if self.add.component_observers.contains_key(&component_id) {
            flags.insert(ArchetypeFlags::ON_ADD_OBSERVER);
        }

        if self.insert.component_observers.contains_key(&component_id) {
            flags.insert(ArchetypeFlags::ON_INSERT_OBSERVER);
        }

        if self.replace.component_observers.contains_key(&component_id) {
            flags.insert(ArchetypeFlags::ON_REPLACE_OBSERVER);
        }

        if self.remove.component_observers.contains_key(&component_id) {
            flags.insert(ArchetypeFlags::ON_REMOVE_OBSERVER);
        }

        if self.despawn.component_observers.contains_key(&component_id) {
            flags.insert(ArchetypeFlags::ON_DESPAWN_OBSERVER);
        }
    }
}

/// Collection of [`ObserverRunner`] for [`Observer`] registered to a particular event.
///
/// This is stored inside of [`Observers`], specialized for each kind of observer.
#[derive(Default, Debug)]
pub struct CachedObservers {
    // Observers listening for any time this event is fired, regardless of target
    // This will also respond to events targeting specific components or entities
    pub(super) universal_observers: ObserverMap,
    // Observers listening for this trigger fired at a specific component
    pub(super) component_observers: HashMap<ComponentId, CachedComponentObservers>,
    // Observers listening for this trigger fired at a specific entity
    pub(super) entity_observers: EntityHashMap<ObserverMap>,
}

impl CachedObservers {
    /// Registers an observer for this event type by parsing the [`ObserverDescriptor`].
    fn register_observer(
        &mut self,
        observer_entity: Entity,
        observer_runner: ObserverRunner,
        descriptor: &ObserverDescriptor,
    ) {
        if descriptor.is_universal() {
            // Universal observers
            self.universal_observers
                .insert(observer_entity, observer_runner.clone());

            // Universal component observers
            for &component_id in descriptor.components() {
                let component_observers = self.component_observers.entry(component_id).or_default();
                component_observers
                    .universal_observers
                    .insert(observer_entity, observer_runner.clone());
            }
        } else {
            // Non-component entity observers
            for &targeted_entity in descriptor.entities() {
                let targeted_entity_observers =
                    self.entity_observers.entry(targeted_entity).or_default();
                targeted_entity_observers.insert(observer_entity, observer_runner.clone());
            }

            // Component-entity observers
            for &component_id in descriptor.components() {
                let component_observers = self.component_observers.entry(component_id).or_default();

                for &targeted_entity in descriptor.entities() {
                    let entity_component_observers = component_observers
                        .entity_component_observers
                        .entry(targeted_entity)
                        .or_default();
                    entity_component_observers.insert(observer_entity, observer_runner.clone());
                }
            }
        }
    }

    /// Unregisters an observer for this event type by parsing the [`ObserverDescriptor`].
    fn unregister_observer(&mut self, observer_entity: Entity, descriptor: &ObserverDescriptor) {
        if descriptor.is_universal() {
            // Universal observers
            self.universal_observers.remove(&observer_entity);

            // Universal component observers
            for &component_id in descriptor.components() {
                if let Some(component_observers) = self.component_observers.get_mut(&component_id) {
                    component_observers
                        .universal_observers
                        .remove(&observer_entity);
                }
            }
        } else {
            // Non-component entity observers
            for targeted_entity in descriptor.entities() {
                if let Some(observers_for_targeted_entity) =
                    self.entity_observers.get_mut(targeted_entity)
                {
                    observers_for_targeted_entity.remove(&observer_entity);
                }
            }

            // Component-entity observers
            for component_id in descriptor.components() {
                if let Some(component_observers) = self.component_observers.get_mut(component_id) {
                    for targeted_entity in descriptor.entities() {
                        if let Some(targeted_entity_component_observers) = component_observers
                            .entity_component_observers
                            .get_mut(targeted_entity)
                        {
                            targeted_entity_component_observers.remove(&observer_entity);
                        }
                    }
                }
            }
        }
    }

    /// Returns the observers listening for this trigger, regardless of target.
    /// These observers will also respond to events targeting specific components or entities.
    pub fn universal_observers(&self) -> &ObserverMap {
        &self.universal_observers
    }

    /// Returns the observers listening for this trigger targeting components.
    pub fn get_component_observers(&self) -> &HashMap<ComponentId, CachedComponentObservers> {
        &self.component_observers
    }

    /// Returns the observers listening for this trigger targeting entities.
    pub fn entity_observers(&self) -> &HashMap<ComponentId, CachedComponentObservers> {
        &self.component_observers
    }
}

/// Map between an observer entity and its [`ObserverRunner`]
pub type ObserverMap = EntityHashMap<ObserverRunner>;

/// Collection of [`ObserverRunner`] for [`Observer`] registered to a particular event targeted at a specific component.
///
/// This is stored inside of [`CachedObservers`].
#[derive(Default, Debug)]
pub struct CachedComponentObservers {
    // Observers listening to events targeting this component, but not a specific entity
    pub(super) universal_observers: ObserverMap,
    // Observers listening to events targeting this component on a specific entity
    pub(super) entity_component_observers: EntityHashMap<ObserverMap>,
}

impl CachedComponentObservers {
    /// Returns the observers listening for this trigger, regardless of target.
    /// These observers will also respond to events targeting specific entities.
    pub fn global_observers(&self) -> &ObserverMap {
        &self.universal_observers
    }

    /// Returns the observers listening for this trigger targeting this component on a specific entity.
    pub fn entity_component_observers(&self) -> &EntityHashMap<ObserverMap> {
        &self.entity_component_observers
    }
}
