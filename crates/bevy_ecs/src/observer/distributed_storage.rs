//! Information about observers that is stored on the entities themselves.
//!
//! This allows for easier cleanup, better inspection, and more flexible querying.
//!
//! Each observer is associated with an entity, defined by the [`Observer`] component.
//! The [`Observer`] component contains the system that will be run when the observer is triggered,
//! and the [`ObserverDescriptor`] which contains information about what the observer is observing.
//!
//! When we watch entities, we add the [`ObservedBy`] component to those entities,
//! which links back to the observer entity.

use core::any::Any;

use crate::{
    component::ComponentId,
    error::{ErrorContext, ErrorHandler},
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
pub struct Observer {
    pub(crate) error_handler: Option<ErrorHandler>,
    pub(crate) system: Box<dyn AnyNamedSystem>,
    pub(crate) descriptor: ObserverDescriptor,
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
            descriptor: Default::default(),
            error_handler: None,
            runner: observer_system_runner::<E, B, I::System>,
            last_trigger_id: 0,
        }
    }

    /// Creates a new [`Observer`] with custom runner, this is mostly used for dynamic event observer
    pub fn with_dynamic_runner(runner: ObserverRunner) -> Self {
        Self {
            system: Box::new(IntoSystem::into_system(|| {})),
            descriptor: Default::default(),
            error_handler: None,
            runner,
            last_trigger_id: 0,
        }
    }

    /// Observe the given `entity`. This will cause the [`Observer`] to run whenever the [`Event`] is triggered
    /// for the `entity`.
    pub fn with_entity(mut self, entity: Entity) -> Self {
        self.descriptor.entities.push(entity);
        self
    }

    /// Observe the given `entity`. This will cause the [`Observer`] to run whenever the [`Event`] is triggered
    /// for the `entity`.
    /// Note that if this is called _after_ an [`Observer`] is spawned, it will produce no effects.
    pub fn watch_entity(&mut self, entity: Entity) {
        self.descriptor.entities.push(entity);
    }

    /// Observe the given `component`. This will cause the [`Observer`] to run whenever the [`Event`] is triggered
    /// with the given component target.
    pub fn with_component(mut self, component: ComponentId) -> Self {
        self.descriptor.components.push(component);
        self
    }

    /// Observe the given `event`. This will cause the [`Observer`] to run whenever an event with the given [`ComponentId`]
    /// is triggered.
    /// # Safety
    /// The type of the `event` [`ComponentId`] _must_ match the actual value
    /// of the event passed into the observer system.
    pub unsafe fn with_event(mut self, event: ComponentId) -> Self {
        self.descriptor.events.push(event);
        self
    }

    /// Set the error handler to use for this observer.
    ///
    /// See the [`error` module-level documentation](crate::error) for more information.
    pub fn with_error_handler(mut self, error_handler: fn(BevyError, ErrorContext)) -> Self {
        self.error_handler = Some(error_handler);
        self
    }

    /// Returns the [`ObserverDescriptor`] for this [`Observer`].
    pub fn descriptor(&self) -> &ObserverDescriptor {
        &self.descriptor
    }

    /// Returns the name of the [`Observer`]'s system .
    pub fn system_name(&self) -> DebugName {
        self.system.system_name()
    }
}

/// Store information about what an [`Observer`] observes.
///
/// This information is stored inside of the [`Observer`] component,
#[derive(Default, Clone)]
pub struct ObserverDescriptor {
    /// The events the observer is watching.
    pub(super) events: Vec<ComponentId>,

    /// The components the observer is watching.
    pub(super) components: Vec<ComponentId>,

    /// The entities the observer is watching.
    pub(super) entities: Vec<Entity>,
}

impl ObserverDescriptor {
    /// Add the given `events` to the descriptor.
    /// # Safety
    /// The type of each [`ComponentId`] in `events` _must_ match the actual value
    /// of the event passed into the observer.
    pub unsafe fn with_events(mut self, events: Vec<ComponentId>) -> Self {
        self.events = events;
        self
    }

    /// Add the given `components` to the descriptor.
    pub fn with_components(mut self, components: Vec<ComponentId>) -> Self {
        self.components = components;
        self
    }

    /// Add the given `entities` to the descriptor.
    pub fn with_entities(mut self, entities: Vec<Entity>) -> Self {
        self.entities = entities;
        self
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
