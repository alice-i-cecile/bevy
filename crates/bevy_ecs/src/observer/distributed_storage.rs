//! Information about observers that is stored on the entities themselves.
//!
//! This allows for easier cleanup, better inspection, and more flexible querying.
//!
//! Each observer is associated with an entity, defined by the [`Observer`] component.
//! The [`Observer`] component contains the system that will be run when the observer is triggered,
//! and the [`ObserverDescriptor`] which contains information about what the observer is observing.
//!
//! The [`ObserverDescriptor`] acts as the ultimate source of truth for "what an observer observes",
//! and is fed into the [centralized storage](crate::observer::centralized_storage) for fast indexes.
//! This synchronization is done by the [`ObserverDescriptor::on_insert`] and [`ObserverDescriptor::on_replace`] hooks,
//! which are called whenever the [`ObserverDescriptor`] is modified due the immutable components pattern.
//!
//! When we watch entities, we add the [`ObservedBy`] component to those entities,
//! which links back to the observer entity.

use alloc::vec;
use core::any::Any;
use log::warn;

use crate::{
    component::{ComponentId, Immutable, StorageType},
    error::{ErrorContext, ErrorHandler},
    lifecycle::ComponentHook,
    observer::{observer_system_runner, ObserverRunner},
    prelude::*,
    system::IntoObserverSystem,
};
use alloc::boxed::Box;
use alloc::vec::Vec;
use bevy_utils::prelude::DebugName;

#[cfg(feature = "bevy_reflect")]
use crate::reflect::{ReflectComponent, ReflectFromWorld};
#[cfg(all(feature = "serialize", feature = "bevy_reflect"))]
use bevy_reflect::{ReflectDeserialize, ReflectSerialize};

/// An [`Observer`] system. Add this [`Component`] to an [`Entity`] to turn it into an "observer".
///
/// Observers listen for a "trigger" of a specific [`Event`]. An event can be triggered on the [`World`]
/// by calling [`World::trigger`], or if the event is an [`EntityEvent`], it can also be triggered for specific
/// entity targets using [`World::trigger_targets`].
///
/// Note that [`BufferedEvent`]s sent using [`EventReader`] and [`EventWriter`] are _not_ automatically triggered.
/// They must be triggered at a specific point in the schedule.
///
/// # Usage
///
/// The simplest usage of the observer pattern looks like this:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # let mut world = World::default();
/// #[derive(Event)]
/// struct Speak {
///     message: String,
/// }
///
/// world.add_observer(|trigger: On<Speak>| {
///     println!("{}", trigger.event().message);
/// });
///
/// // Observers currently require a flush() to be registered. In the context of schedules,
/// // this will generally be done for you.
/// world.flush();
///
/// world.trigger(Speak {
///     message: "Hello!".into(),
/// });
/// ```
///
/// Notice that we used [`World::add_observer`]. This is just a shorthand for spawning an [`Observer`] manually:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # let mut world = World::default();
/// # #[derive(Event)]
/// # struct Speak;
/// // These are functionally the same:
/// world.add_observer(|trigger: On<Speak>| {});
/// world.spawn(Observer::new(|trigger: On<Speak>| {}));
/// ```
///
/// Observers are systems. They can access arbitrary [`World`] data by adding [`SystemParam`]s:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # let mut world = World::default();
/// # #[derive(Event)]
/// # struct PrintNames;
/// # #[derive(Component, Debug)]
/// # struct Name;
/// world.add_observer(|trigger: On<PrintNames>, names: Query<&Name>| {
///     for name in &names {
///         println!("{name:?}");
///     }
/// });
/// ```
///
/// Note that [`On`] must always be the first parameter.
///
/// You can also add [`Commands`], which means you can spawn new entities, insert new components, etc:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # let mut world = World::default();
/// # #[derive(Event)]
/// # struct SpawnThing;
/// # #[derive(Component, Debug)]
/// # struct Thing;
/// world.add_observer(|trigger: On<SpawnThing>, mut commands: Commands| {
///     commands.spawn(Thing);
/// });
/// ```
///
/// Observers can also trigger new events:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # let mut world = World::default();
/// # #[derive(Event)]
/// # struct A;
/// # #[derive(Event)]
/// # struct B;
/// world.add_observer(|trigger: On<A>, mut commands: Commands| {
///     commands.trigger(B);
/// });
/// ```
///
/// When the commands are flushed (including these "nested triggers") they will be
/// recursively evaluated until there are no commands left, meaning nested triggers all
/// evaluate at the same time!
///
/// If the event is an [`EntityEvent`], it can be triggered for specific entities,
/// which will be passed to the [`Observer`]:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # let mut world = World::default();
/// # let entity = world.spawn_empty().id();
/// #[derive(Event, EntityEvent)]
/// struct Explode;
///
/// world.add_observer(|trigger: On<Explode>, mut commands: Commands| {
///     println!("Entity {} goes BOOM!", trigger.target());
///     commands.entity(trigger.target()).despawn();
/// });
///
/// world.flush();
///
/// world.trigger_targets(Explode, entity);
/// ```
///
/// You can trigger multiple entities at once:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # let mut world = World::default();
/// # let e1 = world.spawn_empty().id();
/// # let e2 = world.spawn_empty().id();
/// # #[derive(Event, EntityEvent)]
/// # struct Explode;
/// world.trigger_targets(Explode, [e1, e2]);
/// ```
///
/// Observers can also watch _specific_ entities, which enables you to assign entity-specific logic:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # #[derive(Component, Debug)]
/// # struct Name(String);
/// # let mut world = World::default();
/// # let e1 = world.spawn_empty().id();
/// # let e2 = world.spawn_empty().id();
/// # #[derive(Event, EntityEvent)]
/// # struct Explode;
/// world.entity_mut(e1).observe(|trigger: On<Explode>, mut commands: Commands| {
///     println!("Boom!");
///     commands.entity(trigger.target()).despawn();
/// });
///
/// world.entity_mut(e2).observe(|trigger: On<Explode>, mut commands: Commands| {
///     println!("The explosion fizzles! This entity is immune!");
/// });
/// ```
///
/// Information about which entities / components / events an observer is observing is stored in the [`ObserverDescriptor`] component,
/// which is added to the [`Observer`] entity via required components.
///
/// If all entities watched by a given [`Observer`] are despawned, the [`Observer`] entity will also be despawned.
/// This protects against observer "garbage" building up over time.
///
/// The examples above calling [`EntityWorldMut::observe`] to add entity-specific observer logic are (once again)
/// just shorthand for spawning an [`Observer`] directly:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # let mut world = World::default();
/// # let entity = world.spawn_empty().id();
/// # #[derive(Event, EntityEvent)]
/// # struct Explode;
/// let mut observer = Observer::new(|trigger: On<Explode>| {});
/// observer.watch_entity(entity);
/// world.spawn(observer);
/// ```
///
/// Note that the [`Observer`] component is not added to the entity it is observing. Observers should always be their own entities!
///
/// You can call [`Observer::watch_entity`] more than once, which allows you to watch multiple entities with the same [`Observer`].
/// serves as the "source of truth" of the observer.
///
/// [`SystemParam`]: crate::system::SystemParam
#[derive(Component)]
#[require(ObserverDescriptor)]
pub struct Observer {
    pub(crate) error_handler: Option<ErrorHandler>,
    pub(crate) system: Box<dyn AnyNamedSystem>,
    /// The ID of the event that was last triggered for this observer.
    ///
    /// Used to ensure that observers are not run multiple times for the same event trigger.
    pub(crate) last_trigger_id: u32,
    pub(crate) runner: ObserverRunner,
}

impl Observer {
    /// Creates a new [`Observer`], which defaults to a "global" observer. This means it will run whenever the event `E` is triggered
    /// for _any_ entity (or no entity).
    ///
    /// # Panics
    ///
    /// Panics if the given system is an exclusive system.
    pub fn new<E: Event, B: Bundle, M, I: IntoObserverSystem<E, B, M>>(system: I) -> Self {
        let system = Box::new(IntoObserverSystem::into_system(system));
        assert!(
            !system.is_exclusive(),
            concat!(
                "Exclusive system `{}` may not be used as observer.\n",
                "Instead of `&mut World`, use either `DeferredWorld` if you do not need structural changes, or `Commands` if you do."
            ),
            system.name()
        );
        Self {
            system,
            error_handler: None,
            runner: observer_system_runner::<E, B, I::System>,
            last_trigger_id: 0,
        }
    }

    /// Creates a new [`Observer`] with custom runner, this is mostly used for dynamic event observer
    pub fn with_dynamic_runner(runner: ObserverRunner) -> Self {
        Self {
            system: Box::new(IntoSystem::into_system(|| {})),
            error_handler: None,
            runner,
            last_trigger_id: 0,
        }
    }

    /// Set the error handler to use for this observer.
    ///
    /// See the [`error` module-level documentation](crate::error) for more information.
    pub fn with_error_handler(mut self, error_handler: fn(BevyError, ErrorContext)) -> Self {
        self.error_handler = Some(error_handler);
        self
    }

    /// Returns the name of the [`Observer`]'s system .
    pub fn system_name(&self) -> DebugName {
        self.system.system_name()
    }
}

/// Store information about what an [`Observer`] observes.
///
/// If the set of entities watched is empty, this is a "universal" observer,
/// and all matching events will trigger the observer,
/// regardless of the target entity.
///
/// This component is required by the [`Observer`] component to track what it is observing.
#[derive(Default, Clone)]
pub struct ObserverDescriptor {
    /// The events the observer is watching.
    pub(super) events: Vec<ComponentId>,

    /// The components the observer is watching.
    pub(super) components: Vec<ComponentId>,

    /// The entities the observer is watching.
    pub(super) entities: Vec<Entity>,
}

// Manual implementation to avoid putting too much complexity
// into the derive macro with custom hooks
impl Component for ObserverDescriptor {
    const STORAGE_TYPE: StorageType = StorageType::Table;

    // We're using the immutable components pattern to ensure reliable data synchronization
    // between the distributed storage and the centralized storage.
    type Mutability = Immutable;

    // TODO: we're using Commands here to modify the centralized storage,
    // because we cannot soundly mutate the centralized storage directly from DeferredWorld
    // due to safety requriments in Observers::invoke.
    // This introduces a small performance penalty, and more importantly, delays the evaluation
    // of the synchronization.
    fn on_insert() -> Option<ComponentHook> {
        Some(|mut deferred_world, hook_context| {
            let observer_entity = hook_context.entity;

            deferred_world.commands().register_observer(observer_entity);
        })
    }

    fn on_replace() -> Option<ComponentHook> {
        Some(|mut deferred_world, hook_context| {
            let observer_entity = hook_context.entity;

            // We need to clone out the entity descriptor here / now,
            // rather than fetching it from the world when the command is applied,
            // because the ObserverDescriptor will have been removed by the time
            // the command is applied.
            let observer_descriptor = deferred_world
                .get::<ObserverDescriptor>(observer_entity)
                .unwrap()
                .clone();

            deferred_world
                .commands()
                .unregister_observer(observer_entity, observer_descriptor);
        })
    }
}

impl Commands<'_, '_> {
    /// Registers the observer entity in the centralized [`Observers`] storage.
    // The call signature does not match the unregister_observer variant,
    // as we can simply fetch the `observer_descriptor` from the world
    pub(crate) fn register_observer(&mut self, observer_entity: Entity) {
        self.queue(move |world: &mut World| {
            let Some(observer_descriptor) = world.get::<ObserverDescriptor>(observer_entity) else {
                // Fail quietly if the observer descriptor is not present;
                // we shouldn't register observers that have been somehow removed.
                warn!(
                    "Observer entity {} does not have an ObserverDescriptor component. \
                     This observer will not be registered.",
                    observer_entity
                );

                return;
            };

            let Some(observer) = world.get::<Observer>(observer_entity) else {
                // Fail quietly if the observer is not present;
                // we shouldn't register observers that have been somehow removed.
                warn!(
                    "Observer entity {} does not have an Observer component. \
                     This observer will not be registered.",
                    observer_entity
                );

                return;
            };

            // Clone to avoid aliasing borrows
            let observer_descriptor = observer_descriptor.clone();
            let observer_runner = observer.runner.clone();

            world.observers.register_observer(
                observer_entity,
                observer_runner,
                &observer_descriptor.clone(),
            );
        });
    }

    /// Unregisters the observer entity from the centralized [`Observers`] storage.
    ///
    /// The `observer_descriptor` should be gathered from a hook or observer
    /// that runs at the time of the [`ObserverDescriptor`]'s removal.
    pub(crate) fn unregister_observer(
        &mut self,
        observer_entity: Entity,
        observer_descriptor: ObserverDescriptor,
    ) {
        self.queue(move |world: &mut World| {
            world
                .observers
                .unregister_observer(observer_entity, &observer_descriptor);
        });
    }
}

impl ObserverDescriptor {
    /// Create a new [`ObserverDescriptor`] that watches no events, components, or entities.
    pub const fn universal() -> Self {
        Self {
            events: Vec::new(),
            components: Vec::new(),
            entities: Vec::new(),
        }
    }

    /// Creates a new [`ObserverDescriptor`] that watches the given `event`.
    ///
    /// # Safety
    /// The type of the `event` _must_ match the actual value
    /// of the event passed into the observer.
    pub unsafe fn from_event(event: ComponentId) -> Self {
        Self {
            events: vec![event],
            components: Vec::new(),
            entities: Vec::new(),
        }
    }

    /// Creates a new [`ObserverDescriptor`] that watches the given `component`.
    pub fn from_component(component: ComponentId) -> Self {
        Self {
            events: Vec::new(),
            components: vec![component],
            entities: Vec::new(),
        }
    }

    /// Creates a new [`ObserverDescriptor`] that watches the given `entity`.
    pub fn from_entity(entity: Entity) -> Self {
        Self {
            events: Vec::new(),
            components: Vec::new(),
            entities: vec![entity],
        }
    }

    /// Adds the given `event` to the descriptor.
    ///
    /// # Safety
    /// The type of the `event` _must_ match the actual value
    /// of the event passed into the observer.
    pub unsafe fn watch_event(&mut self, event: ComponentId) {
        self.events.push(event);
    }

    /// Sets the list of watched `events`.
    ///
    /// # Safety
    /// The type of each [`ComponentId`] in `events` _must_ match the actual value
    /// of the event passed into the observer.
    pub unsafe fn set_events(mut self, events: Vec<ComponentId>) -> Self {
        self.events = events;
        self
    }

    /// Adds the given `component` to the descriptor.
    pub fn watch_component(&mut self, component: ComponentId) {
        self.components.push(component);
    }

    /// Sets the list of watched `components`.
    pub fn set_components(mut self, components: Vec<ComponentId>) -> Self {
        self.components = components;
        self
    }

    /// Adds the given `entity` to the descriptor.
    pub fn watch_entity(&mut self, entity: Entity) {
        self.entities.push(entity);
    }

    /// Sets the list of watched `entities`.
    pub fn set_entities(mut self, entities: Vec<Entity>) -> Self {
        self.entities = entities;
        self
    }

    /// Checks if this observer is a "universal" observer,
    /// meaning it does not watch any specific entities.
    pub fn is_universal(&self) -> bool {
        self.entities.is_empty()
    }

    /// Returns the `events` that the observer is watching.
    pub fn events(&self) -> &[ComponentId] {
        &self.events
    }

    /// Returns the `components` that the observer is watching.
    pub fn components(&self) -> &[ComponentId] {
        &self.components
    }

    /// Returns the `entities` that the observer is watching.
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }
}

pub(crate) trait AnyNamedSystem: Any + Send + Sync + 'static {
    fn system_name(&self) -> DebugName;
}

impl<T: Any + System> AnyNamedSystem for T {
    fn system_name(&self) -> DebugName {
        self.name()
    }
}

/// A [`Relation`] that tracks the single entity that this bespoke observer is watching.
///
/// This is used to link the observer to the entity it is observing, allowing for easier cleanup and inspection.
///
/// Its counterpart is [`ObservedBy`], which tracks the observers that are observing a given entity.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
#[cfg_attr(
    feature = "bevy_reflect",
    reflect(Component, PartialEq, Debug, FromWorld, Clone)
)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    all(feature = "serialize", feature = "bevy_reflect"),
    reflect(Serialize, Deserialize)
)]
#[relationship(relationship_target = ObservedBy)]
pub struct ObserverOf(pub Entity);

// TODO: We need to impl either FromWorld or Default so ObserverOf can be registered as Reflect.
// This is because Reflect deserialize by creating an instance and apply a patch on top.
// However ObserverOf should only ever be set with a real user-defined entity.  Its worth looking into
// better ways to handle cases like this.
impl FromWorld for ObserverOf {
    #[inline(always)]
    fn from_world(_world: &mut World) -> Self {
        ObserverOf(Entity::PLACEHOLDER)
    }
}

/// Tracks a list of entity observers for the [`Entity`] [`ObservedBy`] is added to.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = ObserverOf)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
#[cfg_attr(feature = "bevy_reflect", reflect(Component, Debug))]
pub struct ObservedBy(Vec<Entity>);

impl ObservedBy {
    /// Provides a read-only reference to the list of entities observing this entity.
    pub fn get(&self) -> &[Entity] {
        &self.0
    }
}
