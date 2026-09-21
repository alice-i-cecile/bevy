use crate::{
    archetype::Archetype,
    bundle::{
        Bundle, BundleFromComponents, BundleId, BundleInserter, BundleRemover, DynamicBundle,
        InsertMode,
    },
    change_detection::{ComponentTicks, MaybeLocation, MutUntyped, Tick},
    component::{
        Component, ComponentId, Components, Mutable, RequiredComponentsScratch, StorageType,
    },
    entity::{Entity, EntityCloner, EntityClonerBuilder, EntityLocation, OptIn, OptOut},
    error::{BevyError, ErrorContext, Result},
    event::{EntityComponentsTrigger, EntityEvent},
    lifecycle::{DespawnEvent, DiscardEvent, RemoveEvent, DESPAWN, DISCARD, REMOVE},
    observer::IntoEntityObserver,
    query::{
        has_conflicts, DebugCheckedUnwrap, QueryAccessError, ReadOnlyQueryData,
        ReleaseStateQueryData, SingleEntityQueryData,
    },
    relationship::RelationshipHookMode,
    resource::{Resource, ResourceEntities},
    storage::{SparseSets, Table},
    system::EntityCommands,
    template::{InsertingComponents, SceneEntityReferences, Template, TemplateContext},
    world::{
        error::EntityComponentError, unsafe_world_cell::UnsafeEntityCell, ComponentEntry,
        DynamicComponentFetch, EntityMut, EntityRef, FilteredEntityMut, FilteredEntityRef, Mut,
        OccupiedComponentEntry, Ref, VacantComponentEntry, World,
    },
};

use alloc::{format, vec::Vec};
use bevy_ptr::{move_as_ptr, MovingPtr, OwningPtr};
use bevy_utils::prelude::DebugName;
use core::{any::TypeId, marker::PhantomData, mem::MaybeUninit, ptr::NonNull};

/// A mutable reference to a particular [`Entity`], and the entire world.
///
/// This is essentially a performance-optimized `(Entity, &mut World)` tuple,
/// which caches the [`EntityLocation`] to reduce duplicate lookups.
///
/// Since this type provides mutable access to the entire world, only one
/// [`EntityWorldMut`] can exist at a time for a given world.
///
/// See also [`EntityMut`], which allows disjoint mutable access to multiple
/// entities at once.  Unlike `EntityMut`, this type allows adding and
/// removing components, and despawning the entity.
///
/// # Invariants and Risk
///
/// An [`EntityWorldMut`] may point to a despawned entity.
/// You can check this via [`is_despawned`](Self::is_despawned).
/// Using an [`EntityWorldMut`] of a despawned entity may panic in some contexts, so read method documentation carefully.
///
/// Unless you have strong reason to assume these invariants, you should generally avoid keeping an [`EntityWorldMut`] to an entity that is potentially not spawned.
/// For example, when inserting a component, that component insert may trigger an observer that despawns the entity.
/// So, when you don't have full knowledge of what commands may interact with this entity,
/// do not further use this value without first checking [`is_despawned`](Self::is_despawned).
pub struct EntityWorldMut<'w> {
    world: &'w mut World,
    entity: Entity,
    location: Option<EntityLocation>,
}

impl<'w> EntityWorldMut<'w> {
    #[track_caller]
    #[inline(never)]
    #[cold]
    fn panic_despawned(&self) -> ! {
        panic!(
            "Entity {} {}",
            self.entity,
            self.world.entities().get_spawned(self.entity).unwrap_err()
        );
    }

    #[inline(always)]
    #[track_caller]
    pub(crate) fn assert_not_despawned(&self) {
        if self.location.is_none() {
            self.panic_despawned()
        }
    }

    #[inline(always)]
    fn as_unsafe_entity_cell_readonly(&self) -> UnsafeEntityCell<'_> {
        let location = self.location();
        let last_change_tick = self.world.last_change_tick;
        let change_tick = self.world.read_change_tick();
        UnsafeEntityCell::new(
            self.world.as_unsafe_world_cell_readonly(),
            self.entity,
            location,
            last_change_tick,
            change_tick,
        )
    }

    #[inline(always)]
    fn as_unsafe_entity_cell(&mut self) -> UnsafeEntityCell<'_> {
        let location = self.location();
        let last_change_tick = self.world.last_change_tick;
        let change_tick = self.world.change_tick();
        UnsafeEntityCell::new(
            self.world.as_unsafe_world_cell(),
            self.entity,
            location,
            last_change_tick,
            change_tick,
        )
    }

    #[inline(always)]
    fn into_unsafe_entity_cell(self) -> UnsafeEntityCell<'w> {
        let location = self.location();
        let last_change_tick = self.world.last_change_tick;
        let change_tick = self.world.change_tick();
        UnsafeEntityCell::new(
            self.world.as_unsafe_world_cell(),
            self.entity,
            location,
            last_change_tick,
            change_tick,
        )
    }

    /// # Safety
    ///
    ///  The `location` must be sourced from `world`'s `Entities` and must exactly match the location for `entity`.
    ///  If the `entity` is not spawned for any reason (See [`EntityNotSpawnedError`](crate::entity::EntityNotSpawnedError)), the location should be `None`.
    ///
    ///  The above is trivially satisfied if `location` was sourced from `world.entities().get_spawned(entity).ok()`.
    #[inline]
    pub(crate) unsafe fn new(
        world: &'w mut World,
        entity: Entity,
        location: Option<EntityLocation>,
    ) -> Self {
        debug_assert_eq!(world.entities().get_spawned(entity).ok(), location);

        EntityWorldMut {
            world,
            entity,
            location,
        }
    }

    /// Consumes `self` and returns read-only access to all of the entity's
    /// components, with the world `'w` lifetime.
    pub fn into_readonly(self) -> EntityRef<'w> {
        // SAFETY:
        // - We have exclusive access to the entire world.
        // - Consuming `self` ensures no mutable accesses are active.
        unsafe { EntityRef::new(self.into_unsafe_entity_cell()) }
    }

    /// Gets read-only access to all of the entity's components.
    #[inline]
    pub fn as_readonly(&self) -> EntityRef<'_> {
        // SAFETY:
        // - We have exclusive access to the entire world.
        // - `&self` ensures no mutable accesses are active.
        unsafe { EntityRef::new(self.as_unsafe_entity_cell_readonly()) }
    }

    /// Consumes `self` and returns non-structural mutable access to all of the
    /// entity's components, with the world `'w` lifetime.
    pub fn into_mutable(self) -> EntityMut<'w> {
        // SAFETY:
        // - We have exclusive access to the entire world.
        // - Consuming `self` ensures there are no other accesses.
        unsafe { EntityMut::new(self.into_unsafe_entity_cell()) }
    }

    /// Gets non-structural mutable access to all of the entity's components.
    #[inline]
    pub fn as_mutable(&mut self) -> EntityMut<'_> {
        // SAFETY:
        // - We have exclusive access to the entire world.
        // - `&mut self` ensures there are no other accesses.
        unsafe { EntityMut::new(self.as_unsafe_entity_cell()) }
    }

    /// Returns the [ID](Entity) of the current entity.
    #[inline]
    #[must_use = "Omit the .id() call if you do not need to store the `Entity` identifier."]
    pub fn id(&self) -> Entity {
        self.entity
    }

    /// Gets metadata indicating the location where the current entity is stored.
    #[inline]
    pub fn try_location(&self) -> Option<EntityLocation> {
        self.location
    }

    /// Returns if the entity is spawned or not.
    #[inline]
    pub fn is_spawned(&self) -> bool {
        self.try_location().is_some()
    }

    /// Returns the archetype that the current entity belongs to.
    #[inline]
    pub fn try_archetype(&self) -> Option<&Archetype> {
        self.try_location()
            .map(|location| &self.world.archetypes[location.archetype_id])
    }

    /// Gets metadata indicating the location where the current entity is stored.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn location(&self) -> EntityLocation {
        match self.try_location() {
            Some(a) => a,
            None => self.panic_despawned(),
        }
    }

    /// Returns the archetype that the current entity belongs to.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn archetype(&self) -> &Archetype {
        match self.try_archetype() {
            Some(a) => a,
            None => self.panic_despawned(),
        }
    }

    /// Returns `true` if the current entity has a component of type `T`.
    /// Otherwise, this returns `false`.
    ///
    /// ## Notes
    ///
    /// If you do not know the concrete type of a component, consider using
    /// [`Self::contains_id`] or [`Self::contains_type_id`].
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn contains<T: Component>(&self) -> bool {
        self.contains_type_id(TypeId::of::<T>())
    }

    /// Returns `true` if the current entity has a component identified by `component_id`.
    /// Otherwise, this returns false.
    ///
    /// ## Notes
    ///
    /// - If you know the concrete type of the component, you should prefer [`Self::contains`].
    /// - If you know the component's [`TypeId`] but not its [`ComponentId`], consider using
    ///   [`Self::contains_type_id`].
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn contains_id(&self, component_id: ComponentId) -> bool {
        self.as_unsafe_entity_cell_readonly()
            .contains_id(component_id)
    }

    /// Returns `true` if the current entity has a component with the type identified by `type_id`.
    /// Otherwise, this returns false.
    ///
    /// ## Notes
    ///
    /// - If you know the concrete type of the component, you should prefer [`Self::contains`].
    /// - If you have a [`ComponentId`] instead of a [`TypeId`], consider using [`Self::contains_id`].
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn contains_type_id(&self, type_id: TypeId) -> bool {
        self.as_unsafe_entity_cell_readonly()
            .contains_type_id(type_id)
    }

    /// Gets access to the component of type `T` for the current entity.
    /// Returns `None` if the entity does not have a component of type `T`.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn get<T: Component>(&self) -> Option<&'_ T> {
        self.as_readonly().get()
    }

    /// Returns read-only components for the current entity that match the query `Q`.
    ///
    /// # Panics
    ///
    /// If the entity does not have the components required by the query `Q` or if the entity
    /// has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn components<Q: ReadOnlyQueryData + ReleaseStateQueryData + SingleEntityQueryData>(
        &self,
    ) -> Q::Item<'_, 'static> {
        self.as_readonly().components::<Q>()
    }

    /// Returns read-only components for the current entity that match the query `Q`,
    /// or `None` if the entity does not have the components required by the query `Q`.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn get_components<Q: ReadOnlyQueryData + ReleaseStateQueryData + SingleEntityQueryData>(
        &self,
    ) -> Result<Q::Item<'_, 'static>, QueryAccessError> {
        self.as_readonly().get_components::<Q>()
    }

    /// Returns components for the current entity that match the query `Q`,
    /// or `None` if the entity does not have the components required by the query `Q`.
    ///
    /// # Example
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// #
    /// #[derive(Component)]
    /// struct X(usize);
    /// #[derive(Component)]
    /// struct Y(usize);
    ///
    /// # let mut world = World::default();
    /// let mut entity = world.spawn((X(0), Y(0)));
    /// // Get mutable access to two components at once
    /// // SAFETY: X and Y are different components
    /// let (mut x, mut y) =
    ///     unsafe { entity.get_components_mut_unchecked::<(&mut X, &mut Y)>() }.unwrap();
    /// *x = X(1);
    /// *y = Y(1);
    /// // This would trigger undefined behavior, as the `&mut X`s would alias:
    /// // entity.get_components_mut_unchecked::<(&mut X, &mut X)>();
    /// ```
    ///
    /// # Safety
    /// It is the caller's responsibility to ensure that
    /// the `QueryData` does not provide aliasing mutable references to the same component.
    ///
    /// /// # See also
    ///
    /// - [`Self::get_components_mut`] for the safe version that performs aliasing checks
    pub unsafe fn get_components_mut_unchecked<Q: ReleaseStateQueryData + SingleEntityQueryData>(
        &mut self,
    ) -> Result<Q::Item<'_, 'static>, QueryAccessError> {
        // SAFETY: Caller the `QueryData` does not provide aliasing mutable references to the same component
        unsafe { self.as_mutable().into_components_mut_unchecked::<Q>() }
    }

    /// Returns components for the current entity that match the query `Q`.
    /// In the case of conflicting [`QueryData`](crate::query::QueryData), unregistered components, or missing components,
    /// this will return a [`QueryAccessError`]
    ///
    /// # Example
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// #
    /// #[derive(Component)]
    /// struct X(usize);
    /// #[derive(Component)]
    /// struct Y(usize);
    ///
    /// # let mut world = World::default();
    /// let mut entity = world.spawn((X(0), Y(0))).into_mutable();
    /// // Get mutable access to two components at once
    /// // SAFETY: X and Y are different components
    /// let (mut x, mut y) = entity.get_components_mut::<(&mut X, &mut Y)>().unwrap();
    /// ```
    ///
    /// Note that this does an O(n^2) check that the [`QueryData`](crate::query::QueryData) does not conflict. If performance is a
    /// consideration you should use [`Self::get_components_mut_unchecked`] instead.
    pub fn get_components_mut<Q: ReleaseStateQueryData + SingleEntityQueryData>(
        &mut self,
    ) -> Result<Q::Item<'_, 'static>, QueryAccessError> {
        self.as_mutable().into_components_mut::<Q>()
    }

    /// Consumes self and returns components for the current entity that match the query `Q` for the world lifetime `'w`,
    /// or `None` if the entity does not have the components required by the query `Q`.
    ///
    /// # Example
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// #
    /// #[derive(Component)]
    /// struct X(usize);
    /// #[derive(Component)]
    /// struct Y(usize);
    ///
    /// # let mut world = World::default();
    /// let mut entity = world.spawn((X(0), Y(0)));
    /// // Get mutable access to two components at once
    /// // SAFETY: X and Y are different components
    /// let (mut x, mut y) =
    ///     unsafe { entity.into_components_mut_unchecked::<(&mut X, &mut Y)>() }.unwrap();
    /// *x = X(1);
    /// *y = Y(1);
    /// // This would trigger undefined behavior, as the `&mut X`s would alias:
    /// // entity.into_components_mut_unchecked::<(&mut X, &mut X)>();
    /// ```
    ///
    /// # Safety
    /// It is the caller's responsibility to ensure that
    /// the `QueryData` does not provide aliasing mutable references to the same component.
    ///
    /// # See also
    ///
    /// - [`Self::into_components_mut`] for the safe version that performs aliasing checks
    pub unsafe fn into_components_mut_unchecked<
        Q: ReleaseStateQueryData + SingleEntityQueryData,
    >(
        self,
    ) -> Result<Q::Item<'w, 'static>, QueryAccessError> {
        // SAFETY: Caller the `QueryData` does not provide aliasing mutable references to the same component
        unsafe { self.into_mutable().into_components_mut_unchecked::<Q>() }
    }

    /// Consumes self and returns components for the current entity that match the query `Q` for the world lifetime `'w`,
    /// or `None` if the entity does not have the components required by the query `Q`.
    ///
    /// The checks for aliasing mutable references may be expensive.
    /// If performance is a concern, consider making multiple calls to [`Self::get_mut`].
    /// If that is not possible, consider using [`Self::into_components_mut_unchecked`] to skip the checks.
    ///
    /// # Panics
    ///
    /// - If the `QueryData` provides aliasing mutable references to the same component.
    /// - If the entity has been despawned while this `EntityWorldMut` is still alive.
    ///
    /// # Example
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// #
    /// #[derive(Component)]
    /// struct X(usize);
    /// #[derive(Component)]
    /// struct Y(usize);
    ///
    /// # let mut world = World::default();
    /// let mut entity = world.spawn((X(0), Y(0)));
    /// // Get mutable access to two components at once
    /// let (mut x, mut y) = entity.into_components_mut::<(&mut X, &mut Y)>().unwrap();
    /// *x = X(1);
    /// *y = Y(1);
    /// ```
    ///
    /// ```should_panic
    /// # use bevy_ecs::prelude::*;
    /// #
    /// # #[derive(Component)]
    /// # struct X(usize);
    /// #
    /// # let mut world = World::default();
    /// let mut entity = world.spawn((X(0)));
    /// // This panics, as the `&mut X`s would alias:
    /// entity.into_components_mut::<(&mut X, &mut X)>();
    /// ```
    pub fn into_components_mut<Q: ReleaseStateQueryData + SingleEntityQueryData>(
        self,
    ) -> Result<Q::Item<'w, 'static>, QueryAccessError> {
        has_conflicts::<Q>(self.world.components())?;
        // SAFETY: we checked that there were not conflicting components above
        unsafe { self.into_mutable().into_components_mut_unchecked::<Q>() }
    }

    /// Consumes `self` and gets access to the component of type `T` with
    /// the world `'w` lifetime for the current entity.
    /// Returns `None` if the entity does not have a component of type `T`.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn into_borrow<T: Component>(self) -> Option<&'w T> {
        self.into_readonly().get()
    }

    /// Gets access to the component of type `T` for the current entity,
    /// including change detection information as a [`Ref`].
    ///
    /// Returns `None` if the entity does not have a component of type `T`.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn get_ref<T: Component>(&self) -> Option<Ref<'_, T>> {
        self.as_readonly().get_ref()
    }

    /// Consumes `self` and gets access to the component of type `T`
    /// with the world `'w` lifetime for the current entity,
    /// including change detection information as a [`Ref`].
    ///
    /// Returns `None` if the entity does not have a component of type `T`.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn into_ref<T: Component>(self) -> Option<Ref<'w, T>> {
        self.into_readonly().get_ref()
    }

    /// Gets mutable access to the component of type `T` for the current entity.
    /// Returns `None` if the entity does not have a component of type `T`.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn get_mut<T: Component<Mutability = Mutable>>(&mut self) -> Option<Mut<'_, T>> {
        self.as_mutable().into_mut()
    }

    /// Temporarily removes a [`Component`] `T` from this [`Entity`] and runs the
    /// provided closure on it, returning the result if `T` was available.
    /// This will trigger the `Remove` and `Discard` component hooks without
    /// causing an archetype move.
    ///
    /// This is most useful with immutable components, where removal and reinsertion
    /// is the only way to modify a value.
    ///
    /// If you do not need to ensure the above hooks are triggered, and your component
    /// is mutable, prefer using [`get_mut`](EntityWorldMut::get_mut).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use bevy_ecs::prelude::*;
    /// #
    /// #[derive(Component, PartialEq, Eq, Debug)]
    /// #[component(immutable)]
    /// struct Foo(bool);
    ///
    /// # let mut world = World::default();
    /// # world.register_component::<Foo>();
    /// #
    /// # let entity = world.spawn(Foo(false)).id();
    /// #
    /// # let mut entity = world.entity_mut(entity);
    /// #
    /// # assert_eq!(entity.get::<Foo>(), Some(&Foo(false)));
    /// #
    /// entity.modify_component(|foo: &mut Foo| {
    ///     foo.0 = true;
    /// });
    /// #
    /// # assert_eq!(entity.get::<Foo>(), Some(&Foo(true)));
    /// ```
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn modify_component<T: Component, R>(&mut self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.assert_not_despawned();

        let result = self
            .world
            .modify_component(self.entity, f)
            .expect("entity access must be valid")?;

        self.update_location();

        Some(result)
    }

    /// Temporarily removes a [`Component`] `T` from this [`Entity`] and runs the
    /// provided closure on it, returning the result if `T` was available.
    /// This will trigger the `Remove` and `Discard` component hooks without
    /// causing an archetype move.
    ///
    /// This is most useful with immutable components, where removal and reinsertion
    /// is the only way to modify a value.
    ///
    /// If you do not need to ensure the above hooks are triggered, and your component
    /// is mutable, prefer using [`get_mut`](EntityWorldMut::get_mut).
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn modify_component_by_id<R>(
        &mut self,
        component_id: ComponentId,
        f: impl for<'a> FnOnce(MutUntyped<'a>) -> R,
    ) -> Option<R> {
        self.assert_not_despawned();

        let result = self
            .world
            .modify_component_by_id(self.entity, component_id, f)
            .expect("entity access must be valid")?;

        self.update_location();

        Some(result)
    }

    /// Gets mutable access to the component of type `T` for the current entity.
    /// Returns `None` if the entity does not have a component of type `T`.
    ///
    /// # Safety
    ///
    /// - `T` must be a mutable component
    #[inline]
    pub unsafe fn get_mut_assume_mutable<T: Component>(&mut self) -> Option<Mut<'_, T>> {
        let entity_mut = self.as_mutable();
        // SAFETY: Same preconditions
        unsafe { entity_mut.into_mut_assume_mutable() }
    }

    /// Consumes `self` and gets mutable access to the component of type `T`
    /// with the world `'w` lifetime for the current entity.
    /// Returns `None` if the entity does not have a component of type `T`.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn into_mut<T: Component<Mutability = Mutable>>(self) -> Option<Mut<'w, T>> {
        // SAFETY: consuming `self` implies exclusive access
        unsafe { self.into_unsafe_entity_cell().get_mut() }
    }

    /// Consumes `self` and gets mutable access to the component of type `T`
    /// with the world `'w` lifetime for the current entity.
    /// Returns `None` if the entity does not have a component of type `T`.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    ///
    /// # Safety
    ///
    /// - `T` must be a mutable component
    #[inline]
    pub unsafe fn into_mut_assume_mutable<T: Component>(self) -> Option<Mut<'w, T>> {
        // SAFETY: consuming `self` implies exclusive access
        unsafe { self.into_unsafe_entity_cell().get_mut_assume_mutable() }
    }

    /// Gets a reference to the resource of the given type
    ///
    /// # Panics
    ///
    /// Panics if the resource does not exist.
    /// Use [`get_resource`](EntityWorldMut::get_resource) instead if you want to handle this case.
    #[inline]
    #[track_caller]
    pub fn resource<R: Resource>(&self) -> &R {
        self.world.resource::<R>()
    }

    /// Gets a mutable reference to the resource of the given type
    ///
    /// # Panics
    ///
    /// Panics if the resource does not exist.
    /// Use [`get_resource_mut`](World::get_resource_mut) instead if you want to handle this case.
    ///
    /// If you want to instead insert a value if the resource does not exist,
    /// use [`get_resource_or_insert_with`](World::get_resource_or_insert_with).
    #[inline]
    #[track_caller]
    pub fn resource_mut<R: Resource<Mutability = Mutable>>(&mut self) -> Mut<'_, R> {
        self.world.resource_mut::<R>()
    }

    /// Gets a reference to the resource of the given type if it exists
    #[inline]
    pub fn get_resource<R: Resource>(&self) -> Option<&R> {
        self.world.get_resource()
    }

    /// Gets a mutable reference to the resource of the given type if it exists
    #[inline]
    pub fn get_resource_mut<R: Resource<Mutability = Mutable>>(&mut self) -> Option<Mut<'_, R>> {
        self.world.get_resource_mut()
    }

    /// Temporarily removes the requested resource from the [`World`], runs custom user code,
    /// then re-adds the resource before returning.
    ///
    /// # Panics
    ///
    /// Panics if the resource does not exist.
    /// Use [`try_resource_scope`](Self::try_resource_scope) instead if you want to handle this case.
    ///
    /// See [`World::resource_scope`] for further details.
    #[track_caller]
    pub fn resource_scope<R: Resource, U>(
        &mut self,
        f: impl FnOnce(&mut EntityWorldMut, Mut<R>) -> U,
    ) -> U {
        let id = self.id();
        self.world_scope(|world| {
            world.resource_scope(|world, res| {
                // Acquiring a new EntityWorldMut here and using that instead of `self` is fine because
                // the outer `world_scope` will handle updating our location if it gets changed by the user code
                let mut this = world.entity_mut(id);
                f(&mut this, res)
            })
        })
    }

    /// Temporarily removes the requested resource from the [`World`] if it exists, runs custom user code,
    /// then re-adds the resource before returning. Returns `None` if the resource does not exist in the [`World`].
    ///
    /// See [`World::try_resource_scope`] for further details.
    pub fn try_resource_scope<R: Resource, U>(
        &mut self,
        f: impl FnOnce(&mut EntityWorldMut, Mut<R>) -> U,
    ) -> Option<U> {
        let id = self.id();
        self.world_scope(|world| {
            world.try_resource_scope(|world, res| {
                // Acquiring a new EntityWorldMut here and using that instead of `self` is fine because
                // the outer `world_scope` will handle updating our location if it gets changed by the user code
                let mut this = world.entity_mut(id);
                f(&mut this, res)
            })
        })
    }

    /// Retrieves this world's [`ResourceEntities`].
    #[inline]
    #[track_caller]
    pub fn resource_entities(&self) -> &ResourceEntities {
        self.world.resource_entities()
    }

    /// Retrieves the [`Entity`] associated with the resource of type `R`, if it exists.
    #[inline]
    #[track_caller]
    pub fn resource_entity<R: Resource>(&self) -> Option<Entity> {
        self.world.resource_entity::<R>()
    }

    /// Retrieves the change ticks for the given component. This can be useful for implementing change
    /// detection in custom runtimes.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn get_change_ticks<T: Component>(&self) -> Option<ComponentTicks> {
        self.as_readonly().get_change_ticks::<T>()
    }

    /// Get the [`MaybeLocation`] from where the given [`Component`] was last changed from.
    /// This contains information regarding the last place (in code) that changed this component and can be useful for debugging.
    /// For more information, see [`Location`](https://doc.rust-lang.org/nightly/core/panic/struct.Location.html), and enable the `track_location` feature.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn get_changed_by<T: Component>(&self) -> Option<MaybeLocation> {
        self.as_readonly().get_changed_by::<T>()
    }

    /// Retrieves the change ticks for the given [`ComponentId`]. This can be useful for implementing change
    /// detection in custom runtimes.
    ///
    /// **You should prefer to use the typed API [`EntityWorldMut::get_change_ticks`] where possible and only
    /// use this in cases where the actual component types are not known at
    /// compile time.**
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn get_change_ticks_by_id(&self, component_id: ComponentId) -> Option<ComponentTicks> {
        self.as_readonly().get_change_ticks_by_id(component_id)
    }

    /// Returns untyped read-only reference(s) to component(s) for the
    /// current entity, based on the given [`ComponentId`]s.
    ///
    /// **You should prefer to use the typed API [`EntityWorldMut::get`] where
    /// possible and only use this in cases where the actual component types
    /// are not known at compile time.**
    ///
    /// Unlike [`EntityWorldMut::get`], this returns untyped reference(s) to
    /// component(s), and it's the job of the caller to ensure the correct
    /// type(s) are dereferenced (if necessary).
    ///
    /// # Errors
    ///
    /// Returns [`EntityComponentError::MissingComponent`] if the entity does
    /// not have a component.
    ///
    /// # Examples
    ///
    /// For examples on how to use this method, see [`EntityRef::get_by_id`].
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn get_by_id<F: DynamicComponentFetch>(
        &self,
        component_ids: F,
    ) -> Result<F::Ref<'_>, EntityComponentError> {
        self.as_readonly().get_by_id(component_ids)
    }

    /// Consumes `self` and returns untyped read-only reference(s) to
    /// component(s) with lifetime `'w` for the current entity, based on the
    /// given [`ComponentId`]s.
    ///
    /// **You should prefer to use the typed API [`EntityWorldMut::into_borrow`]
    /// where possible and only use this in cases where the actual component
    /// types are not known at compile time.**
    ///
    /// Unlike [`EntityWorldMut::into_borrow`], this returns untyped reference(s) to
    /// component(s), and it's the job of the caller to ensure the correct
    /// type(s) are dereferenced (if necessary).
    ///
    /// # Errors
    ///
    /// Returns [`EntityComponentError::MissingComponent`] if the entity does
    /// not have a component.
    ///
    /// # Examples
    ///
    /// For examples on how to use this method, see [`EntityRef::get_by_id`].
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn into_borrow_by_id<F: DynamicComponentFetch>(
        self,
        component_ids: F,
    ) -> Result<F::Ref<'w>, EntityComponentError> {
        self.into_readonly().get_by_id(component_ids)
    }

    /// Returns [untyped mutable reference(s)](MutUntyped) to component(s) for
    /// the current entity, based on the given [`ComponentId`]s.
    ///
    /// **You should prefer to use the typed API [`EntityWorldMut::get_mut`] where
    /// possible and only use this in cases where the actual component types
    /// are not known at compile time.**
    ///
    /// Unlike [`EntityWorldMut::get_mut`], this returns untyped reference(s) to
    /// component(s), and it's the job of the caller to ensure the correct
    /// type(s) are dereferenced (if necessary).
    ///
    /// # Errors
    ///
    /// - Returns [`EntityComponentError::MissingComponent`] if the entity does
    ///   not have a component.
    /// - Returns [`EntityComponentError::AliasedMutability`] if a component
    ///   is requested multiple times.
    ///
    /// # Examples
    ///
    /// For examples on how to use this method, see [`EntityMut::get_mut_by_id`].
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn get_mut_by_id<F: DynamicComponentFetch>(
        &mut self,
        component_ids: F,
    ) -> Result<F::Mut<'_>, EntityComponentError> {
        self.as_mutable().into_mut_by_id(component_ids)
    }

    /// Returns [untyped mutable reference(s)](MutUntyped) to component(s) for
    /// the current entity, based on the given [`ComponentId`]s.
    /// Assumes the given [`ComponentId`]s refer to mutable components.
    ///
    /// **You should prefer to use the typed API [`EntityWorldMut::get_mut_assume_mutable`] where
    /// possible and only use this in cases where the actual component types
    /// are not known at compile time.**
    ///
    /// Unlike [`EntityWorldMut::get_mut_assume_mutable`], this returns untyped reference(s) to
    /// component(s), and it's the job of the caller to ensure the correct
    /// type(s) are dereferenced (if necessary).
    ///
    /// # Errors
    ///
    /// - Returns [`EntityComponentError::MissingComponent`] if the entity does
    ///   not have a component.
    /// - Returns [`EntityComponentError::AliasedMutability`] if a component
    ///   is requested multiple times.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    ///
    /// # Safety
    /// It is the callers responsibility to ensure that
    /// - the provided [`ComponentId`]s must refer to mutable components.
    #[inline]
    pub unsafe fn get_mut_assume_mutable_by_id<F: DynamicComponentFetch>(
        &mut self,
        component_ids: F,
    ) -> Result<F::Mut<'_>, EntityComponentError> {
        // SAFETY: Upheld by caller
        unsafe {
            self.as_mutable()
                .into_mut_assume_mutable_by_id(component_ids)
        }
    }

    /// Consumes `self` and returns [untyped mutable reference(s)](MutUntyped)
    /// to component(s) with lifetime `'w` for the current entity, based on the
    /// given [`ComponentId`]s.
    ///
    /// **You should prefer to use the typed API [`EntityWorldMut::into_mut`] where
    /// possible and only use this in cases where the actual component types
    /// are not known at compile time.**
    ///
    /// Unlike [`EntityWorldMut::into_mut`], this returns untyped reference(s) to
    /// component(s), and it's the job of the caller to ensure the correct
    /// type(s) are dereferenced (if necessary).
    ///
    /// # Errors
    ///
    /// - Returns [`EntityComponentError::MissingComponent`] if the entity does
    ///   not have a component.
    /// - Returns [`EntityComponentError::AliasedMutability`] if a component
    ///   is requested multiple times.
    ///
    /// # Examples
    ///
    /// For examples on how to use this method, see [`EntityMut::get_mut_by_id`].
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[inline]
    pub fn into_mut_by_id<F: DynamicComponentFetch>(
        self,
        component_ids: F,
    ) -> Result<F::Mut<'w>, EntityComponentError> {
        self.into_mutable().into_mut_by_id(component_ids)
    }

    /// Consumes `self` and returns [untyped mutable reference(s)](MutUntyped)
    /// to component(s) with lifetime `'w` for the current entity, based on the
    /// given [`ComponentId`]s.
    /// Assumes the given [`ComponentId`]s refer to mutable components.
    ///
    /// **You should prefer to use the typed API [`EntityWorldMut::into_mut_assume_mutable`] where
    /// possible and only use this in cases where the actual component types
    /// are not known at compile time.**
    ///
    /// Unlike [`EntityWorldMut::into_mut_assume_mutable`], this returns untyped reference(s) to
    /// component(s), and it's the job of the caller to ensure the correct
    /// type(s) are dereferenced (if necessary).
    ///
    /// # Errors
    ///
    /// - Returns [`EntityComponentError::MissingComponent`] if the entity does
    ///   not have a component.
    /// - Returns [`EntityComponentError::AliasedMutability`] if a component
    ///   is requested multiple times.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    ///
    /// # Safety
    /// It is the callers responsibility to ensure that
    /// - the provided [`ComponentId`]s must refer to mutable components.
    #[inline]
    pub unsafe fn into_mut_assume_mutable_by_id<F: DynamicComponentFetch>(
        self,
        component_ids: F,
    ) -> Result<F::Mut<'w>, EntityComponentError> {
        // SAFETY: Upheld by caller
        unsafe {
            self.into_mutable()
                .into_mut_assume_mutable_by_id(component_ids)
        }
    }

    /// Adds a [`Bundle`] of components to the entity.
    ///
    /// This will overwrite any previous value(s) of the same component type.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[track_caller]
    pub fn insert<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        move_as_ptr!(bundle);
        self.insert_with_caller(
            bundle,
            InsertMode::Replace,
            MaybeLocation::caller(),
            RelationshipHookMode::Run,
        )
    }

    /// Adds a [`Bundle`] of components to the entity.
    /// [`Relationship`](crate::relationship::Relationship) components in the bundle will follow the configuration
    /// in `relationship_hook_mode`.
    ///
    /// This will overwrite any previous value(s) of the same component type.
    ///
    /// # Warning
    ///
    /// This can easily break the integrity of relationships. This is intended to be used for cloning and spawning code internals,
    /// not most user-facing scenarios.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[track_caller]
    pub fn insert_with_relationship_hook_mode<T: Bundle>(
        &mut self,
        bundle: T,
        relationship_hook_mode: RelationshipHookMode,
    ) -> &mut Self {
        move_as_ptr!(bundle);
        self.insert_with_caller(
            bundle,
            InsertMode::Replace,
            MaybeLocation::caller(),
            relationship_hook_mode,
        )
    }

    /// Adds a [`Bundle`] of components to the entity without overwriting.
    ///
    /// This will leave any previous value(s) of the same component type
    /// unchanged.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[track_caller]
    pub fn insert_if_new<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        move_as_ptr!(bundle);
        self.insert_with_caller(
            bundle,
            InsertMode::Keep,
            MaybeLocation::caller(),
            RelationshipHookMode::Run,
        )
    }

    /// Adds a [`Bundle`] of components to the entity.
    ///
    /// If one of its required templates fails to build, nothing is inserted and the error is
    /// passed to the world's fallback error handler.
    #[inline]
    pub(crate) fn insert_with_caller<T: Bundle>(
        &mut self,
        bundle: MovingPtr<'_, T>,
        mode: InsertMode,
        caller: MaybeLocation,
        relationship_hook_mode: RelationshipHookMode,
    ) -> &mut Self {
        if let Err(error) =
            self.try_insert_with_caller(bundle, mode, caller, relationship_hook_mode)
        {
            self.report_required_template_error(error, DebugName::type_name::<T>());
        }
        self
    }

    /// Adds a [`Bundle`] of components to the entity.
    ///
    /// If one of its required templates fails to build, nothing is inserted and the error is returned.
    #[inline]
    pub(crate) fn try_insert_with_caller<T: Bundle>(
        &mut self,
        bundle: MovingPtr<'_, T>,
        mode: InsertMode,
        caller: MaybeLocation,
        relationship_hook_mode: RelationshipHookMode,
    ) -> Result {
        let location = self.location();
        let change_tick = self.world.change_tick();
        let bundle_id = self.world.register_bundle_info::<T>();
        // SAFETY:
        // - `bundle_id` was just registered
        // - `location.archetype_id` is part of a valid `EntityLocation`.
        let Ok(mut bundle_inserter) = (unsafe {
            BundleInserter::new_with_id(self.world, location.archetype_id, bundle_id, change_tick)
        }) else {
            return self.insert_with_required_templates(
                bundle_id,
                bundle,
                mode,
                caller,
                relationship_hook_mode,
            );
        };
        // SAFETY:
        // - `location` matches current entity and thus must currently exist in the source
        //   archetype for this inserter and its location within the archetype.
        // - `T` matches the type used to create the `BundleInserter`.
        // - `apply_effect` is called exactly once after this function.
        // - The value pointed at by `bundle` is not accessed for anything other than `apply_effect`
        //   and the caller ensures that the value is not accessed or dropped after this function
        //   returns.
        let (bundle, location) = bundle.partial_move(|bundle| unsafe {
            bundle_inserter.insert(
                self.entity,
                location,
                bundle,
                mode,
                caller,
                relationship_hook_mode,
            )
        });
        self.location = Some(location);
        self.world.flush();
        self.update_location();
        // SAFETY:
        // - This is called exactly once after the `BundleInsert::insert` call before returning to safe code.
        // - `bundle` points to the same `B` that `BundleInsert::insert` was called on.
        unsafe { T::apply_effect(bundle, self) };
        Ok(())
    }

    /// Inserts a dynamic [`Component`] into the entity.
    ///
    /// This will overwrite any previous value(s) of the same component type.
    ///
    /// You should prefer to use the typed API [`EntityWorldMut::insert`] where possible.
    ///
    /// # Safety
    ///
    /// - [`ComponentId`] must be from the same world as [`EntityWorldMut`]
    /// - [`OwningPtr`] must be a valid reference to the type represented by [`ComponentId`]
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[track_caller]
    pub unsafe fn insert_by_id(
        &mut self,
        component_id: ComponentId,
        component: OwningPtr<'_>,
    ) -> &mut Self {
        // SAFETY: Upheld by caller
        unsafe {
            self.insert_by_id_with_caller(
                component_id,
                component,
                InsertMode::Replace,
                MaybeLocation::caller(),
                RelationshipHookMode::Run,
            )
        }
    }

    /// # Safety
    /// - [`OwningPtr`] must be a valid reference to the type represented by [`ComponentId`]
    #[inline]
    pub(crate) unsafe fn insert_by_id_with_caller(
        &mut self,
        component_id: ComponentId,
        component: OwningPtr<'_>,
        mode: InsertMode,
        caller: MaybeLocation,
        relationship_hook_insert_mode: RelationshipHookMode,
    ) -> &mut Self {
        let location = self.location();
        let change_tick = self.world.change_tick();
        let bundle_id = self.world.bundles.init_component_info(
            &mut self.world.storages,
            &self.world.components,
            component_id,
        );
        // SAFETY:
        // init done above via init_component_info
        let storage_type = unsafe { self.world.bundles.get_storage_unchecked(bundle_id) };

        // SAFETY:
        // - bundle initialized above
        // - archetype id taken from existing entity
        let Ok(bundle_inserter) = (unsafe {
            BundleInserter::new_with_id(self.world, location.archetype_id, bundle_id, change_tick)
        }) else {
            // SAFETY: the caller upholds the preconditions of `insert_ptrs_with_required_templates`
            let result = unsafe {
                self.insert_ptrs_with_required_templates(
                    bundle_id,
                    &[component_id],
                    core::iter::once(component),
                    mode,
                    caller,
                    relationship_hook_insert_mode,
                )
            };
            if let Err(error) = result {
                self.report_required_template_error(error, DebugName::borrowed("dynamic bundle"));
            }
            return self;
        };

        // SAFETY:
        // - only one component, with its component & storage type retrieved above
        // - entity & location both belong to self
        self.location = Some(unsafe {
            insert_dynamic_bundle(
                bundle_inserter,
                self.entity,
                location,
                Some(component).into_iter(),
                Some(storage_type).iter().cloned(),
                mode,
                caller,
                relationship_hook_insert_mode,
            )
        });
        self.world.flush();
        self.update_location();
        self
    }

    /// Inserts a dynamic [`Bundle`] into the entity.
    ///
    /// This will overwrite any previous value(s) of the same component type.
    ///
    /// You should prefer to use the typed API [`EntityWorldMut::insert`] where possible.
    /// If your [`Bundle`] only has one component, use the cached API [`EntityWorldMut::insert_by_id`].
    ///
    /// If possible, pass a sorted slice of `ComponentId` to maximize caching potential.
    ///
    /// # Safety
    /// - Each [`ComponentId`] must be from the same world as [`EntityWorldMut`]
    /// - Each [`OwningPtr`] must be a valid reference to the type represented by [`ComponentId`]
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[track_caller]
    pub unsafe fn insert_by_ids<'a, I: Iterator<Item = OwningPtr<'a>>>(
        &mut self,
        component_ids: &[ComponentId],
        iter_components: I,
    ) -> &mut Self {
        // SAFETY:
        // same preconditions
        unsafe {
            self.insert_by_ids_internal(component_ids, iter_components, RelationshipHookMode::Run)
        }
    }

    /// # Safety
    /// see [`EntityWorldMut::insert_by_ids`]
    #[track_caller]
    pub(crate) unsafe fn insert_by_ids_internal<'a, I: Iterator<Item = OwningPtr<'a>>>(
        &mut self,
        component_ids: &[ComponentId],
        iter_components: I,
        relationship_hook_insert_mode: RelationshipHookMode,
    ) -> &mut Self {
        // SAFETY: the caller upholds the preconditions of `try_insert_by_ids_internal`
        let result = unsafe {
            self.try_insert_by_ids_internal(
                component_ids,
                iter_components,
                InsertMode::Replace,
                MaybeLocation::caller(),
                relationship_hook_insert_mode,
            )
        };
        if let Err(error) = result {
            self.report_required_template_error(error, DebugName::borrowed("dynamic bundle"));
        }
        self
    }

    /// Like [`Self::insert_by_ids_internal`], but returns the error if one of the required templates fails to build.
    ///
    /// # Safety
    /// see [`EntityWorldMut::insert_by_ids`]
    #[track_caller]
    pub(crate) unsafe fn try_insert_by_ids_internal<'a, I: Iterator<Item = OwningPtr<'a>>>(
        &mut self,
        component_ids: &[ComponentId],
        iter_components: I,
        mode: InsertMode,
        caller: MaybeLocation,
        relationship_hook_insert_mode: RelationshipHookMode,
    ) -> Result {
        let location = self.location();
        let change_tick = self.world.change_tick();
        let bundle_id = self.world.bundles.init_dynamic_info(
            &mut self.world.storages,
            &self.world.components,
            component_ids,
        );

        // SAFETY:
        // init done above via init_dynamic_info
        let mut storage_types =
            core::mem::take(unsafe { self.world.bundles.get_storages_unchecked(bundle_id) });
        // SAFETY:
        // - bundle initialized above
        // - archetype id taken from existing entity
        let Ok(bundle_inserter) = (unsafe {
            BundleInserter::new_with_id(self.world, location.archetype_id, bundle_id, change_tick)
        }) else {
            // SAFETY: restoring the storage types taken above
            *unsafe { self.world.bundles.get_storages_unchecked(bundle_id) } = storage_types;
            // SAFETY: the caller upholds the preconditions of `insert_ptrs_with_required_templates`
            return unsafe {
                self.insert_ptrs_with_required_templates(
                    bundle_id,
                    component_ids,
                    iter_components,
                    mode,
                    caller,
                    relationship_hook_insert_mode,
                )
            };
        };

        // SAFETY:
        // - owning pointers are of the component's types per precondition
        // - storage types retrieved above
        // - entity & location both belong to self
        self.location = Some(unsafe {
            insert_dynamic_bundle(
                bundle_inserter,
                self.entity,
                location,
                iter_components,
                (*storage_types).iter().cloned(),
                mode,
                caller,
                relationship_hook_insert_mode,
            )
        });
        // SAFETY:
        // same as above
        *unsafe { self.world.bundles.get_storages_unchecked(bundle_id) } =
            core::mem::take(&mut storage_types);
        self.world.flush();
        self.update_location();
        Ok(())
    }

    /// The slow path of [`Self::try_insert_with_caller`], used when the bundle adds required components
    /// that are built from templates.
    ///
    /// The bundle's components are moved into scratch storage first, so the templates can read them
    /// through [`TemplateContext::inserting`]. If building fails, the bundle's effect is leaked,
    /// as it cannot be dropped without being applied.
    #[cold]
    #[inline(never)]
    fn insert_with_required_templates<T: Bundle>(
        &mut self,
        bundle_id: BundleId,
        bundle: MovingPtr<'_, T>,
        mode: InsertMode,
        caller: MaybeLocation,
        relationship_hook_mode: RelationshipHookMode,
    ) -> Result {
        let mut scratch = self
            .world
            .required_templates
            .scratch
            .pop()
            .unwrap_or_default();
        let RequiredComponentsScratch {
            alloc,
            explicit_ids,
            explicit_ptrs,
            layouts,
            ..
        } = &mut scratch;
        // SAFETY: the caller registered `bundle_id` for `T`
        let bundle_info = unsafe { self.world.bundles.get_unchecked(bundle_id) };
        explicit_ids.extend_from_slice(bundle_info.explicit_components());
        layouts.extend(
            explicit_ids
                .iter()
                // SAFETY: bundle component ids are valid
                .map(|&id| unsafe { self.world.components.get_info_unchecked(id) }.layout()),
        );
        // SAFETY:
        // - `get_components` is called exactly once, and `apply_effect` is called at most once afterwards
        // - components are written in bundle order, which matches `explicit_ids` and `layouts`
        let (bundle, ()) = bundle.partial_move(|bundle| unsafe {
            T::get_components(bundle, &mut |_, component| {
                let layout = layouts[explicit_ptrs.len()];
                let ptr = alloc.alloc_layout(layout);
                core::ptr::copy_nonoverlapping(component.as_ptr(), ptr.as_ptr(), layout.size());
                explicit_ptrs.push(ptr);
            });
        });

        // SAFETY: the scratch holds the owned components of the bundle registered as `bundle_id`
        let result = unsafe {
            self.insert_scratch_with_required_templates(
                bundle_id,
                &mut scratch,
                mode,
                caller,
                relationship_hook_mode,
            )
        };
        scratch.clear();
        self.world.required_templates.scratch.push(scratch);
        if result.is_ok() {
            // SAFETY: called exactly once after `get_components`
            unsafe { T::apply_effect(bundle, self) };
        }
        result
    }

    /// Like [`Self::insert_with_required_templates`], for components that are passed by pointer.
    ///
    /// # Safety
    /// - `bundle_id` must be the bundle of `component_ids`, in the same world as this entity
    /// - each pointer must own a valid value of the matching component, which is moved into the world or dropped
    #[cold]
    #[inline(never)]
    unsafe fn insert_ptrs_with_required_templates<'a>(
        &mut self,
        bundle_id: BundleId,
        component_ids: &[ComponentId],
        components: impl Iterator<Item = OwningPtr<'a>>,
        mode: InsertMode,
        caller: MaybeLocation,
        relationship_hook_mode: RelationshipHookMode,
    ) -> Result {
        let mut scratch = self
            .world
            .required_templates
            .scratch
            .pop()
            .unwrap_or_default();
        scratch.explicit_ids.extend_from_slice(component_ids);
        scratch
            .explicit_ptrs
            // SAFETY: `OwningPtr`s are never null
            .extend(components.map(|ptr| unsafe { NonNull::new_unchecked(ptr.as_ptr()) }));
        // SAFETY: the caller upholds the preconditions
        let result = unsafe {
            self.insert_scratch_with_required_templates(
                bundle_id,
                &mut scratch,
                mode,
                caller,
                relationship_hook_mode,
            )
        };
        scratch.clear();
        self.world.required_templates.scratch.push(scratch);
        result
    }

    /// Builds every required component that inserting the explicit components in `scratch` would add,
    /// then inserts all of them in a single archetype move.
    ///
    /// On success, every explicit component has been moved into the world. On failure, they have been dropped.
    ///
    /// # Safety
    /// - `bundle_id` must be the bundle of `scratch.explicit_ids`, in the same world as this entity
    /// - each of `scratch.explicit_ptrs` must own a valid value of the matching component
    unsafe fn insert_scratch_with_required_templates(
        &mut self,
        bundle_id: BundleId,
        scratch: &mut RequiredComponentsScratch,
        mode: InsertMode,
        caller: MaybeLocation,
        relationship_hook_mode: RelationshipHookMode,
    ) -> Result {
        // SAFETY: the caller upholds the preconditions
        if let Err(error) = unsafe { self.build_required_components(bundle_id, scratch) } {
            // SAFETY: nothing has been moved out of the scratch
            unsafe {
                self.drop_components(&scratch.explicit_ids, &scratch.explicit_ptrs);
                self.drop_components(&scratch.built_ids, &scratch.built_ptrs);
            }
            self.world.flush();
            self.update_location();
            return Err(error);
        }

        let RequiredComponentsScratch {
            explicit_ids,
            explicit_ptrs,
            built_ids,
            built_ptrs,
            missing,
            write_ids,
            write_ptrs,
            ..
        } = scratch;
        write_ids.extend_from_slice(explicit_ids);
        write_ptrs.extend_from_slice(explicit_ptrs);
        for constructor in missing.iter() {
            let component_id = constructor.component_id();
            let index = built_ids.iter().position(|&id| id == component_id);
            // SAFETY: `build_required_components` built every missing component
            let index = unsafe { index.debug_checked_unwrap() };
            write_ids.push(component_id);
            write_ptrs.push(built_ptrs[index]);
            // It is moved into the world below, so it must not be dropped with the unused ones.
            built_ids.swap_remove(index);
            built_ptrs.swap_remove(index);
        }
        // SAFETY: the remaining built components are not written, and nothing else points to them
        unsafe { self.drop_components(built_ids, built_ptrs) };

        // Every requirement of the write set was built, so this normally takes the fast path. Requirements that were
        // registered after `bundle_id` was cached are missing from its plan, and get built by the slow path here.
        // SAFETY: every pointer owns a valid value of the matching component in `write_ids`, from this world
        unsafe {
            self.try_insert_by_ids_internal(
                write_ids,
                write_ptrs.iter().map(|&ptr| OwningPtr::new(ptr)),
                mode,
                caller,
                relationship_hook_mode,
            )
        }
    }

    /// Builds every required component that inserting `bundle_id` is missing into `scratch`.
    ///
    /// Templates have full world access and can change this entity, which changes which required
    /// components are missing, so this plans the insert again whenever the entity's archetype changes.
    ///
    /// Components that are required by another missing component are built after it, so templates
    /// can read the component that required them through [`TemplateContext::inserting`].
    ///
    /// # Safety
    /// Same as [`Self::insert_scratch_with_required_templates`].
    unsafe fn build_required_components(
        &mut self,
        bundle_id: BundleId,
        scratch: &mut RequiredComponentsScratch,
    ) -> Result {
        'plan: loop {
            let Some(location) = self.location else {
                return Err("Entity was despawned while building its required components".into());
            };
            let change_tick = self.world.change_tick();
            // SAFETY: the caller ensures `bundle_id` is valid, and the archetype id is the entity's
            let plan = unsafe {
                BundleInserter::plan(self.world, location.archetype_id, bundle_id, change_tick)
            };
            scratch.missing.clear();
            scratch
                .missing
                .extend_from_slice(plan.required_components());

            let RequiredComponentsScratch {
                alloc,
                explicit_ids,
                explicit_ptrs,
                built_ids,
                built_ptrs,
                missing,
                ..
            } = &mut *scratch;
            // Values need no context, so build them first to make them visible to every template. Then build
            // templates in reverse, so a template can read the component that required it.
            let values = missing
                .iter()
                .filter(|constructor| !constructor.is_template());
            let templates = missing
                .iter()
                .rev()
                .filter(|constructor| constructor.is_template());
            for constructor in values.chain(templates) {
                let component_id = constructor.component_id();
                if built_ids.contains(&component_id) {
                    continue;
                }
                if self.location.map(|location| location.archetype_id)
                    != Some(location.archetype_id)
                {
                    continue 'plan;
                }
                let key = (self.entity, component_id);
                let is_template = constructor.is_template();
                if is_template && self.world.required_templates.building.contains(&key) {
                    let name = self.world.components.get_name(component_id).unwrap();
                    return Err(format!(
                        "Required component {name} on entity {} requires itself while being built",
                        self.entity
                    )
                    .into());
                }
                let result = {
                    let guard = BuildingGuard::new(self, is_template.then_some(key));
                    let mut entity_references = SceneEntityReferences::default();
                    let mut context = TemplateContext::with_inserting(
                        guard.entity,
                        &mut entity_references,
                        InsertingComponents {
                            explicit_ids,
                            explicit_ptrs,
                            built_ids,
                            built_ptrs,
                        },
                    );
                    constructor.build(&mut context, alloc)
                };
                let ptr = result.map_err(|error| {
                    let name = self.world.components.get_name(component_id).unwrap();
                    format!(
                        "Failed to build required component {name} for entity {}: {error}",
                        self.entity
                    )
                })?;
                built_ids.push(component_id);
                built_ptrs.push(ptr);
            }
            if self.location.map(|location| location.archetype_id) == Some(location.archetype_id) {
                return Ok(());
            }
        }
    }

    /// # Safety
    /// Each pointer in `ptrs` must own a valid value of the matching component in `component_ids`.
    unsafe fn drop_components(&self, component_ids: &[ComponentId], ptrs: &[NonNull<u8>]) {
        for (&id, &ptr) in component_ids.iter().zip(ptrs) {
            // SAFETY: component ids are valid per precondition
            let info = unsafe { self.world.components.get_info_unchecked(id) };
            if let Some(drop) = info.drop() {
                // SAFETY: `ptr` owns a valid value of this component per precondition
                unsafe { drop(OwningPtr::new(ptr)) };
            }
        }
    }

    #[cold]
    fn report_required_template_error(&self, error: BevyError, name: DebugName) {
        (self.world.fallback_error_handler())(error, ErrorContext::RequiredTemplate { name });
    }

    /// Removes all components in the [`Bundle`] from the entity and returns their previous values.
    ///
    /// **Note:** If the entity does not have every component in the bundle, this method will not
    /// remove any of them.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[must_use]
    #[track_caller]
    pub fn take<T: Bundle + BundleFromComponents>(&mut self) -> Option<T> {
        let location = self.location();
        let entity = self.entity;
        let change_tick = self.world.change_tick();

        let mut remover =
            // SAFETY: The archetype id must be valid since this entity is in it.
            unsafe { BundleRemover::new::<T>(self.world, location.archetype_id, change_tick, true) }?;
        // SAFETY:
        // - The passed location has the same archetype as the remover, since they came from the same location.
        // - `location` was obtained from a valid `Self`.
        let (new_location, result) = unsafe {
            remover.remove(
                entity,
                location,
                MaybeLocation::caller(),
                |sets, table, components, bundle_components| {
                    let mut bundle_components = bundle_components.iter().copied();
                    (
                        false,
                        T::from_components(&mut (sets, table), &mut |(sets, table)| {
                            let component_id = bundle_components.next().unwrap();
                            // SAFETY: the component existed to be removed, so its id must be valid.
                            let component_info = components.get_info_unchecked(component_id);
                            match component_info.storage_type() {
                                StorageType::Table => {
                                    table
                                        .as_mut()
                                        // SAFETY: The table must be valid if the component is in it.
                                        .debug_checked_unwrap()
                                        // SAFETY: The remover is cleaning this up.
                                        .take_component(component_id, location.table_row)
                                }
                                StorageType::SparseSet => sets
                                    .get_mut(component_id)
                                    .unwrap()
                                    .remove_and_forget(entity)
                                    .unwrap(),
                            }
                        }),
                    )
                },
            )
        };
        self.location = Some(new_location);

        self.world.flush();
        self.update_location();
        Some(result)
    }

    /// Removes any components in the [`Bundle`] from the entity.
    ///
    /// See [`EntityCommands::remove`](crate::system::EntityCommands::remove) for more details.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[track_caller]
    pub fn remove<T: Bundle>(&mut self) -> &mut Self {
        self.remove_with_caller::<T>(MaybeLocation::caller())
    }

    #[inline]
    pub(crate) fn remove_with_caller<T: Bundle>(&mut self, caller: MaybeLocation) -> &mut Self {
        let location = self.location();
        let change_tick = self.world.change_tick();

        let Some(mut remover) =
            // SAFETY: The archetype id must be valid since this entity is in it.
            (unsafe { BundleRemover::new::<T>(self.world, location.archetype_id, change_tick, false) })
        else {
            return self;
        };
        // SAFETY:
        // - The remover archetype came from the passed location and the removal can not fail.
        // - `location` was obtained from a valid `Self`.
        let new_location = unsafe {
            remover.remove(
                self.entity,
                location,
                caller,
                BundleRemover::empty_pre_remove,
            )
        }
        .0;

        self.location = Some(new_location);
        self.world.flush();
        self.update_location();
        self
    }

    /// Removes all components in the [`Bundle`] and remove all required components for each component in the bundle
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[track_caller]
    pub fn remove_with_requires<T: Bundle>(&mut self) -> &mut Self {
        self.remove_with_requires_with_caller::<T>(MaybeLocation::caller())
    }

    pub(crate) fn remove_with_requires_with_caller<T: Bundle>(
        &mut self,
        caller: MaybeLocation,
    ) -> &mut Self {
        let location = self.location();
        let bundle_id = self.world.register_contributed_bundle_info::<T>();
        let change_tick = self.world.change_tick();

        // SAFETY: We just created the bundle, and the archetype is valid, since we are in it.
        let Some(mut remover) = (unsafe {
            BundleRemover::new_with_id(
                self.world,
                location.archetype_id,
                bundle_id,
                change_tick,
                false,
            )
        }) else {
            return self;
        };
        // SAFETY:
        // - The remover archetype came from the passed location and the removal can not fail.
        // - `location` was obtained from a valid `Self`.
        let new_location = unsafe {
            remover.remove(
                self.entity,
                location,
                caller,
                BundleRemover::empty_pre_remove,
            )
        }
        .0;

        self.location = Some(new_location);
        self.world.flush();
        self.update_location();
        self
    }

    /// Removes any components except those in the [`Bundle`] (and its Required Components) from the entity.
    ///
    /// See [`EntityCommands::retain`](crate::system::EntityCommands::retain) for more details.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[track_caller]
    pub fn retain<T: Bundle>(&mut self) -> &mut Self {
        self.retain_with_caller::<T>(MaybeLocation::caller())
    }

    #[inline]
    pub(crate) fn retain_with_caller<T: Bundle>(&mut self, caller: MaybeLocation) -> &mut Self {
        let old_location = self.location();
        let retained_bundle = self.world.register_bundle_info::<T>();
        let change_tick = self.world.change_tick();
        let archetypes = &mut self.world.archetypes;

        // SAFETY: `retained_bundle` exists as we just registered it.
        let retained_bundle_info = unsafe { self.world.bundles.get_unchecked(retained_bundle) };
        let old_archetype = &mut archetypes[old_location.archetype_id];

        // PERF: this could be stored in an Archetype Edge
        let to_remove = &old_archetype
            .iter_components()
            .filter(|c| !retained_bundle_info.contributed_components().contains(c))
            .collect::<Vec<_>>();
        let remove_bundle = self.world.bundles.init_dynamic_info(
            &mut self.world.storages,
            &self.world.components,
            to_remove,
        );

        // SAFETY: We just created the bundle, and the archetype is valid, since we are in it.
        let Some(mut remover) = (unsafe {
            BundleRemover::new_with_id(
                self.world,
                old_location.archetype_id,
                remove_bundle,
                change_tick,
                false,
            )
        }) else {
            return self;
        };
        // SAFETY:
        // - The remover archetype came from the passed location and the removal can not fail.
        // - `old_location` was obtained from a valid `Self`.
        let new_location = unsafe {
            remover.remove(
                self.entity,
                old_location,
                caller,
                BundleRemover::empty_pre_remove,
            )
        }
        .0;

        self.location = Some(new_location);
        self.world.flush();
        self.update_location();
        self
    }

    /// Removes a dynamic [`Component`] from the entity if it exists.
    ///
    /// You should prefer to use the typed API [`EntityWorldMut::remove`] where possible.
    ///
    /// # Panics
    ///
    /// Panics if the provided [`ComponentId`] does not exist in the [`World`] or if the
    /// entity has been despawned while this `EntityWorldMut` is still alive.
    #[track_caller]
    pub fn remove_by_id(&mut self, component_id: ComponentId) -> &mut Self {
        self.remove_by_id_with_caller(component_id, MaybeLocation::caller())
    }

    #[inline]
    pub(crate) fn remove_by_id_with_caller(
        &mut self,
        component_id: ComponentId,
        caller: MaybeLocation,
    ) -> &mut Self {
        let location = self.location();
        let change_tick = self.world.change_tick();
        let components = &mut self.world.components;

        let bundle_id = self.world.bundles.init_component_info(
            &mut self.world.storages,
            components,
            component_id,
        );

        // SAFETY: We just created the bundle, and the archetype is valid, since we are in it.
        let Some(mut remover) = (unsafe {
            BundleRemover::new_with_id(
                self.world,
                location.archetype_id,
                bundle_id,
                change_tick,
                false,
            )
        }) else {
            return self;
        };
        // SAFETY:
        // - The remover archetype came from the passed location and the removal can not fail.
        // - `location` was obtained from a valid `Self`.
        let new_location = unsafe {
            remover.remove(
                self.entity,
                location,
                caller,
                BundleRemover::empty_pre_remove,
            )
        }
        .0;

        self.location = Some(new_location);
        self.world.flush();
        self.update_location();
        self
    }

    /// Removes a dynamic bundle from the entity if it exists.
    ///
    /// You should prefer to use the typed API [`EntityWorldMut::remove`] where possible.
    ///
    /// # Panics
    ///
    /// Panics if any of the provided [`ComponentId`]s do not exist in the [`World`] or if the
    /// entity has been despawned while this `EntityWorldMut` is still alive.
    #[track_caller]
    pub fn remove_by_ids(&mut self, component_ids: &[ComponentId]) -> &mut Self {
        self.remove_by_ids_with_caller(
            component_ids,
            MaybeLocation::caller(),
            RelationshipHookMode::Run,
            BundleRemover::empty_pre_remove,
        )
    }

    #[inline]
    pub(crate) fn remove_by_ids_with_caller<T: 'static>(
        &mut self,
        component_ids: &[ComponentId],
        caller: MaybeLocation,
        relationship_hook_mode: RelationshipHookMode,
        pre_remove: impl FnOnce(
            &mut SparseSets,
            Option<&mut Table>,
            &Components,
            &[ComponentId],
        ) -> (bool, T),
    ) -> &mut Self {
        let location = self.location();
        let change_tick = self.world.change_tick();
        let components = &mut self.world.components;

        let bundle_id = self.world.bundles.init_dynamic_info(
            &mut self.world.storages,
            components,
            component_ids,
        );

        // SAFETY: We just created the bundle, and the archetype is valid, since we are in it.
        let Some(mut remover) = (unsafe {
            BundleRemover::new_with_id(
                self.world,
                location.archetype_id,
                bundle_id,
                change_tick,
                false,
            )
        }) else {
            return self;
        };
        remover.relationship_hook_mode = relationship_hook_mode;
        // SAFETY:
        // - The remover archetype came from the passed location and the removal can not fail.
        // - `location` was obtained from a valid `Self`.
        let new_location = unsafe { remover.remove(self.entity, location, caller, pre_remove) }.0;

        self.location = Some(new_location);
        self.world.flush();
        self.update_location();
        self
    }

    /// Removes all components associated with the entity.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    #[track_caller]
    pub fn clear(&mut self) -> &mut Self {
        self.clear_with_caller(MaybeLocation::caller())
    }

    #[inline]
    pub(crate) fn clear_with_caller(&mut self, caller: MaybeLocation) -> &mut Self {
        let location = self.location();
        let change_tick = self.world.change_tick();

        // PERF: this should not be necessary
        let component_ids: Vec<ComponentId> = self.archetype().components().to_vec();
        let components = &mut self.world.components;

        let bundle_id = self.world.bundles.init_dynamic_info(
            &mut self.world.storages,
            components,
            component_ids.as_slice(),
        );

        // SAFETY: We just created the bundle, and the archetype is valid, since we are in it.
        let Some(mut remover) = (unsafe {
            BundleRemover::new_with_id(
                self.world,
                location.archetype_id,
                bundle_id,
                change_tick,
                false,
            )
        }) else {
            return self;
        };
        // SAFETY:
        // - The remover archetype came from the passed location and the removal can not fail.
        // - `location` was obtained from a valid `Self`.
        let new_location = unsafe {
            remover.remove(
                self.entity,
                location,
                caller,
                BundleRemover::empty_pre_remove,
            )
        }
        .0;

        self.location = Some(new_location);
        self.world.flush();
        self.update_location();
        self
    }

    /// Despawns the entity without freeing it to the allocator.
    /// This returns the new [`Entity`], which you must manage.
    /// Note that this still increases the generation to differentiate different spawns of the same row.
    ///
    /// Additionally, keep in mind the limitations documented in the type-level docs.
    /// Unless you have full knowledge of this [`EntityWorldMut`]'s lifetime,
    /// you may not assume that nothing else has taken responsibility of this [`Entity`].
    /// If you are not careful, this could cause a double free.
    ///
    /// This may be later [`spawn_at`](World::spawn_at).
    /// See [`World::despawn_no_free`] for details and usage examples.
    #[track_caller]
    pub fn despawn_no_free(mut self) -> Entity {
        self.despawn_no_free_with_caller(MaybeLocation::caller());
        self.entity
    }

    /// Creates a new [`TemplateContext`] for this entity and passes it into the given `func`.
    pub fn template_context<T>(
        &mut self,
        func: impl FnOnce(&mut TemplateContext) -> Result<T>,
    ) -> Result<T> {
        let mut scene_entities = SceneEntityReferences::default();
        let mut context = TemplateContext::new(self, &mut scene_entities);
        func(&mut context)
    }

    /// Builds the given template using a [`TemplateContext`] generated for this entity.
    pub fn build_template<T: Template>(&mut self, template: &T) -> Result<T::Output> {
        self.template_context(|context| template.build_template(context))
    }

    /// This despawns this entity if it is currently spawned, storing the new [`EntityGeneration`](crate::entity::EntityGeneration) in [`Self::entity`] but not freeing it.
    pub(crate) fn despawn_no_free_with_caller(&mut self, caller: MaybeLocation) {
        // setup
        let Some(location) = self.location else {
            // If there is no location, we are already despawned
            return;
        };
        let archetype = &self.world.archetypes[location.archetype_id];

        // SAFETY: Archetype cannot be mutably aliased by DeferredWorld
        let (archetype, mut deferred_world) = unsafe {
            let archetype: *const Archetype = archetype;
            let world = self.world.as_unsafe_world_cell();
            (&*archetype, world.into_deferred())
        };

        // SAFETY: All components in the archetype exist in world
        unsafe {
            if archetype.has_despawn_observer() {
                // SAFETY: the DESPAWN event_key corresponds to the Despawn event's type
                deferred_world.trigger_raw(
                    DESPAWN,
                    &mut DespawnEvent {
                        entity: self.entity,
                    },
                    &mut EntityComponentsTrigger {
                        components: archetype.components(),
                        old_archetype: Some(archetype),
                        new_archetype: None,
                    },
                    caller,
                );
            }
            deferred_world.trigger_on_despawn(
                archetype,
                self.entity,
                archetype.iter_components(),
                caller,
            );
            if archetype.has_discard_observer() {
                // SAFETY: the DISCARD event_key corresponds to the Discard event's type
                deferred_world.trigger_raw(
                    DISCARD,
                    &mut DiscardEvent {
                        entity: self.entity,
                    },
                    &mut EntityComponentsTrigger {
                        components: archetype.components(),
                        old_archetype: Some(archetype),
                        new_archetype: None,
                    },
                    caller,
                );
            }
            deferred_world.trigger_on_discard(
                archetype,
                self.entity,
                archetype.iter_components(),
                caller,
                RelationshipHookMode::Run,
            );
            if archetype.has_remove_observer() {
                // SAFETY: the REMOVE event_key corresponds to the Remove event's type
                deferred_world.trigger_raw(
                    REMOVE,
                    &mut RemoveEvent {
                        entity: self.entity,
                    },
                    &mut EntityComponentsTrigger {
                        components: archetype.components(),
                        old_archetype: Some(archetype),
                        new_archetype: None,
                    },
                    caller,
                );
            }
            deferred_world.trigger_on_remove(
                archetype,
                self.entity,
                archetype.iter_components(),
                caller,
            );
        }

        // do the despawn
        let change_tick = self.world.change_tick();
        for component_id in archetype.components() {
            self.world
                .removed_components
                .write(*component_id, self.entity);
        }
        // SAFETY: Since we had a location, and it was valid, this is safe.
        unsafe {
            let was_at = self
                .world
                .entities
                .update_existing_location(self.entity.index(), None);
            debug_assert_eq!(was_at, Some(location));
            self.world
                .entities
                .mark_spawned_or_despawned(self.entity.index(), caller, change_tick);
        }

        let table_row;
        let moved_entity;
        {
            let archetype = &mut self.world.archetypes[location.archetype_id];
            let remove_result = archetype.swap_remove(location.archetype_row);
            if let Some(swapped_entity) = remove_result.swapped_entity {
                let swapped_location = self.world.entities.get_spawned(swapped_entity).unwrap();
                // SAFETY: swapped_entity is valid and the swapped entity's components are
                // moved to the new location immediately after.
                unsafe {
                    self.world.entities.update_existing_location(
                        swapped_entity.index(),
                        Some(EntityLocation {
                            archetype_id: swapped_location.archetype_id,
                            archetype_row: location.archetype_row,
                            table_id: swapped_location.table_id,
                            table_row: swapped_location.table_row,
                        }),
                    );
                }
            }
            table_row = remove_result.table_row;

            for component_id in archetype.sparse_set_components() {
                // set must have existed for the component to be added.
                let sparse_set = self
                    .world
                    .storages
                    .sparse_sets
                    .get_mut(component_id)
                    .unwrap();
                sparse_set.remove(self.entity);
            }
            // SAFETY: table rows stored in archetypes always exist
            moved_entity = unsafe {
                self.world.storages.tables[archetype.table_id()].swap_remove_unchecked(table_row)
            };
        };

        // Handle displaced entity
        if let Some(moved_entity) = moved_entity {
            let moved_location = self.world.entities.get_spawned(moved_entity).unwrap();
            // SAFETY: `moved_entity` is valid and the provided `EntityLocation` accurately reflects
            //         the current location of the entity and its component data.
            unsafe {
                self.world.entities.update_existing_location(
                    moved_entity.index(),
                    Some(EntityLocation {
                        archetype_id: moved_location.archetype_id,
                        archetype_row: moved_location.archetype_row,
                        table_id: moved_location.table_id,
                        table_row,
                    }),
                );
            }
            self.world.archetypes[moved_location.archetype_id]
                .set_entity_table_row(moved_location.archetype_row, table_row);
        }

        // finish
        // SAFETY: We just despawned it.
        self.entity = unsafe { self.world.entities.mark_free(self.entity.index(), 1) };
        self.world.flush();
    }

    /// Despawns the current entity.
    ///
    /// See [`World::despawn`] for more details.
    ///
    /// # Note
    ///
    /// This will also despawn any [`Children`](crate::hierarchy::Children) entities, and any other [`RelationshipTarget`](crate::relationship::RelationshipTarget) that is configured
    /// to despawn descendants. This results in "recursive despawn" behavior.
    #[track_caller]
    pub fn despawn(self) {
        self.despawn_with_caller(MaybeLocation::caller());
    }

    pub(crate) fn despawn_with_caller(mut self, caller: MaybeLocation) {
        self.despawn_no_free_with_caller(caller);
        if let Ok(None) = self.world.entities.get(self.entity) {
            self.world.entity_allocator.free(self.entity);
        }

        // Otherwise:
        // A command must have reconstructed it (had a location); don't free
        // A command must have already despawned it (err) or otherwise made the free unneeded (ex by spawning and despawning in commands); don't free
    }

    /// Ensures any commands triggered by the actions of Self are applied, equivalent to [`World::flush`]
    pub fn flush(self) -> Entity {
        self.world.flush();
        self.entity
    }

    /// Gets read-only access to the world that the current entity belongs to.
    #[inline]
    pub fn world(&self) -> &World {
        self.world
    }

    /// Returns this entity's world.
    ///
    /// See [`EntityWorldMut::world_scope`] or [`EntityWorldMut::into_world_mut`] for a safe alternative.
    ///
    /// # Safety
    /// Caller must not modify the world in a way that changes the current entity's location
    /// If the caller _does_ do something that could change the location, `self.update_location()`
    /// must be called before using any other methods on this [`EntityWorldMut`].
    #[inline]
    pub unsafe fn world_mut(&mut self) -> &mut World {
        self.world
    }

    /// Returns this entity's [`World`], consuming itself.
    #[inline]
    pub fn into_world_mut(self) -> &'w mut World {
        self.world
    }

    /// Gives mutable access to this entity's [`World`] in a temporary scope.
    /// This is a safe alternative to using [`EntityWorldMut::world_mut`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// #[derive(Resource, Default, Clone, Copy)]
    /// struct R(u32);
    ///
    /// # let mut world = World::new();
    /// # world.init_resource::<R>();
    /// # let mut entity = world.spawn_empty();
    /// // This closure gives us temporary access to the world.
    /// let new_r = entity.world_scope(|world: &mut World| {
    ///     // Mutate the world while we have access to it.
    ///     let mut r = world.resource_mut::<R>();
    ///     r.0 += 1;
    ///
    ///     // Return a value from the world before giving it back to the `EntityWorldMut`.
    ///     *r
    /// });
    /// # assert_eq!(new_r.0, 1);
    /// ```
    pub fn world_scope<U>(&mut self, f: impl FnOnce(&mut World) -> U) -> U {
        struct Guard<'w, 'a> {
            entity_mut: &'a mut EntityWorldMut<'w>,
        }

        impl Drop for Guard<'_, '_> {
            #[inline]
            fn drop(&mut self) {
                self.entity_mut.update_location();
            }
        }

        // When `guard` is dropped at the end of this scope,
        // it will update the cached `EntityLocation` for this instance.
        // This will run even in case the closure `f` unwinds.
        let guard = Guard { entity_mut: self };
        f(guard.entity_mut.world)
    }

    /// Creates a new [`EntityCommands`] instance that writes commands
    /// Use [`EntityWorldMut::flush`] to apply all queued commands
    #[inline]
    pub fn entity_commands(&mut self) -> EntityCommands<'_> {
        let id = self.id();
        EntityCommands {
            entity: id,
            commands: self.world.commands(),
        }
    }

    /// Updates the internal entity location to match the current location in the internal
    /// [`World`].
    ///
    /// This is *only* required when using the unsafe function [`EntityWorldMut::world_mut`],
    /// which enables the location to change.
    ///
    /// Note that if the entity is not spawned for any reason,
    /// this will have a location of `None`, leading some methods to panic.
    pub fn update_location(&mut self) {
        self.location = self.world.entities().get_spawned(self.entity).ok();
    }

    /// Returns if the entity has been despawned.
    ///
    /// Normally it shouldn't be needed to explicitly check if the entity has been despawned
    /// between commands as this shouldn't happen. However, for some special cases where it
    /// is known that a hook or an observer might despawn the entity while a [`EntityWorldMut`]
    /// reference is still held, this method can be used to check if the entity is still alive
    /// to avoid panicking when calling further methods.
    #[inline]
    pub fn is_despawned(&self) -> bool {
        self.location.is_none()
    }

    /// Gets an Entry into the world for this entity and component for in-place manipulation.
    ///
    /// The type parameter specifies which component to get.
    ///
    /// # Examples
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// #[derive(Component, Default, Clone, Copy, Debug, PartialEq)]
    /// struct Comp(u32);
    ///
    /// # let mut world = World::new();
    /// let mut entity = world.spawn_empty();
    /// entity.entry().or_insert_with(|| Comp(4));
    /// # let entity_id = entity.id();
    /// assert_eq!(world.query::<&Comp>().single(&world).unwrap().0, 4);
    ///
    /// # let mut entity = world.get_entity_mut(entity_id).unwrap();
    /// entity.entry::<Comp>().and_modify(|mut c| c.0 += 1);
    /// assert_eq!(world.query::<&Comp>().single(&world).unwrap().0, 5);
    /// ```
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    pub fn entry<'a, T: Component>(&'a mut self) -> ComponentEntry<'w, 'a, T> {
        if self.contains::<T>() {
            ComponentEntry::Occupied(OccupiedComponentEntry {
                entity_world: self,
                _marker: PhantomData,
            })
        } else {
            ComponentEntry::Vacant(VacantComponentEntry {
                entity_world: self,
                _marker: PhantomData,
            })
        }
    }

    /// Creates an [`Observer`](crate::observer::Observer) watching for an [`EntityEvent`] of type `E` whose [`EntityEvent::event_target`]
    /// targets this entity.
    ///
    /// # Panics
    ///
    /// If the entity has been despawned while this `EntityWorldMut` is still alive.
    ///
    /// Panics if the given system is an exclusive system.
    #[track_caller]
    pub fn observe<M>(&mut self, observer: impl IntoEntityObserver<M>) -> &mut Self {
        self.observe_with_caller(observer, MaybeLocation::caller())
    }

    pub(crate) fn observe_with_caller<M>(
        &mut self,
        observer: impl IntoEntityObserver<M>,
        caller: MaybeLocation,
    ) -> &mut Self {
        self.assert_not_despawned();
        let bundle = observer.into_observer_for_entity(self.entity);
        move_as_ptr!(bundle);
        self.world.spawn_with_caller(bundle, caller);
        self.world.flush();
        self.update_location();
        self
    }

    /// Clones parts of an entity (components, observers, etc.) onto another entity,
    /// configured through [`EntityClonerBuilder`].
    ///
    /// The other entity will receive all the components of the original that implement
    /// [`Clone`] or [`Reflect`](bevy_reflect::Reflect) except those that are
    /// [denied](EntityClonerBuilder::deny) in the `config`.
    ///
    /// # Example
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// # #[derive(Component, Clone, PartialEq, Debug)]
    /// # struct ComponentA;
    /// # #[derive(Component, Clone, PartialEq, Debug)]
    /// # struct ComponentB;
    /// # let mut world = World::new();
    /// # let entity = world.spawn((ComponentA, ComponentB)).id();
    /// # let target = world.spawn_empty().id();
    /// // Clone all components except ComponentA onto the target.
    /// world.entity_mut(entity).clone_with_opt_out(target, |builder| {
    ///     builder.deny::<ComponentA>();
    /// });
    /// # assert_eq!(world.get::<ComponentA>(target), None);
    /// # assert_eq!(world.get::<ComponentB>(target), Some(&ComponentB));
    /// ```
    ///
    /// See [`EntityClonerBuilder<OptOut>`] for more options.
    ///
    /// # Panics
    ///
    /// - If this entity has been despawned while this `EntityWorldMut` is still alive.
    /// - If the target entity does not exist.
    pub fn clone_with_opt_out(
        &mut self,
        target: Entity,
        config: impl FnOnce(&mut EntityClonerBuilder<OptOut>) + Send + Sync + 'static,
    ) -> &mut Self {
        self.assert_not_despawned();

        let mut builder = EntityCloner::build_opt_out(self.world);
        config(&mut builder);
        builder.clone_entity(self.entity, target);

        self.world.flush();
        self.update_location();
        self
    }

    /// Clones parts of an entity (components, observers, etc.) onto another entity,
    /// configured through [`EntityClonerBuilder`].
    ///
    /// The other entity will receive only the components of the original that implement
    /// [`Clone`] or [`Reflect`](bevy_reflect::Reflect) and are
    /// [allowed](EntityClonerBuilder::allow) in the `config`.
    ///
    /// # Example
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// # #[derive(Component, Clone, PartialEq, Debug)]
    /// # struct ComponentA;
    /// # #[derive(Component, Clone, PartialEq, Debug)]
    /// # struct ComponentB;
    /// # let mut world = World::new();
    /// # let entity = world.spawn((ComponentA, ComponentB)).id();
    /// # let target = world.spawn_empty().id();
    /// // Clone only ComponentA onto the target.
    /// world.entity_mut(entity).clone_with_opt_in(target, |builder| {
    ///     builder.allow::<ComponentA>();
    /// });
    /// # assert_eq!(world.get::<ComponentA>(target), Some(&ComponentA));
    /// # assert_eq!(world.get::<ComponentB>(target), None);
    /// ```
    ///
    /// See [`EntityClonerBuilder<OptIn>`] for more options.
    ///
    /// # Panics
    ///
    /// - If this entity has been despawned while this `EntityWorldMut` is still alive.
    /// - If the target entity does not exist.
    pub fn clone_with_opt_in(
        &mut self,
        target: Entity,
        config: impl FnOnce(&mut EntityClonerBuilder<OptIn>) + Send + Sync + 'static,
    ) -> &mut Self {
        self.assert_not_despawned();

        let mut builder = EntityCloner::build_opt_in(self.world);
        config(&mut builder);
        builder.clone_entity(self.entity, target);

        self.world.flush();
        self.update_location();
        self
    }

    /// Spawns a clone of this entity and returns the [`Entity`] of the clone.
    ///
    /// The clone will receive all the components of the original that implement
    /// [`Clone`] or [`Reflect`](bevy_reflect::Reflect).
    ///
    /// To configure cloning behavior (such as only cloning certain components),
    /// use [`EntityWorldMut::clone_and_spawn_with_opt_out`]/
    /// [`opt_in`](`EntityWorldMut::clone_and_spawn_with_opt_in`).
    ///
    /// # Panics
    ///
    /// If this entity has been despawned while this `EntityWorldMut` is still alive.
    pub fn clone_and_spawn(&mut self) -> Entity {
        self.clone_and_spawn_with_opt_out(|_| {})
    }

    /// Spawns a clone of this entity and allows configuring cloning behavior
    /// using [`EntityClonerBuilder`], returning the [`Entity`] of the clone.
    ///
    /// The clone will receive all the components of the original that implement
    /// [`Clone`] or [`Reflect`](bevy_reflect::Reflect) except those that are
    /// [denied](EntityClonerBuilder::deny) in the `config`.
    ///
    /// # Example
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// # let mut world = World::new();
    /// # let entity = world.spawn((ComponentA, ComponentB)).id();
    /// # #[derive(Component, Clone, PartialEq, Debug)]
    /// # struct ComponentA;
    /// # #[derive(Component, Clone, PartialEq, Debug)]
    /// # struct ComponentB;
    /// // Create a clone of an entity but without ComponentA.
    /// let entity_clone = world.entity_mut(entity).clone_and_spawn_with_opt_out(|builder| {
    ///     builder.deny::<ComponentA>();
    /// });
    /// # assert_eq!(world.get::<ComponentA>(entity_clone), None);
    /// # assert_eq!(world.get::<ComponentB>(entity_clone), Some(&ComponentB));
    /// ```
    ///
    /// See [`EntityClonerBuilder<OptOut>`] for more options.
    ///
    /// # Panics
    ///
    /// If this entity has been despawned while this `EntityWorldMut` is still alive.
    pub fn clone_and_spawn_with_opt_out(
        &mut self,
        config: impl FnOnce(&mut EntityClonerBuilder<OptOut>) + Send + Sync + 'static,
    ) -> Entity {
        self.assert_not_despawned();
        let entity_clone = self.world.spawn_empty().id();

        let mut builder = EntityCloner::build_opt_out(self.world);
        config(&mut builder);
        builder.clone_entity(self.entity, entity_clone);

        self.world.flush();
        self.update_location();
        entity_clone
    }

    /// Spawns a clone of this entity and allows configuring cloning behavior
    /// using [`EntityClonerBuilder`], returning the [`Entity`] of the clone.
    ///
    /// The clone will receive only the components of the original that implement
    /// [`Clone`] or [`Reflect`](bevy_reflect::Reflect) and are
    /// [allowed](EntityClonerBuilder::allow) in the `config`.
    ///
    /// # Example
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// # let mut world = World::new();
    /// # let entity = world.spawn((ComponentA, ComponentB)).id();
    /// # #[derive(Component, Clone, PartialEq, Debug)]
    /// # struct ComponentA;
    /// # #[derive(Component, Clone, PartialEq, Debug)]
    /// # struct ComponentB;
    /// // Create a clone of an entity but only with ComponentA.
    /// let entity_clone = world.entity_mut(entity).clone_and_spawn_with_opt_in(|builder| {
    ///     builder.allow::<ComponentA>();
    /// });
    /// # assert_eq!(world.get::<ComponentA>(entity_clone), Some(&ComponentA));
    /// # assert_eq!(world.get::<ComponentB>(entity_clone), None);
    /// ```
    ///
    /// See [`EntityClonerBuilder<OptIn>`] for more options.
    ///
    /// # Panics
    ///
    /// If this entity has been despawned while this `EntityWorldMut` is still alive.
    pub fn clone_and_spawn_with_opt_in(
        &mut self,
        config: impl FnOnce(&mut EntityClonerBuilder<OptIn>) + Send + Sync + 'static,
    ) -> Entity {
        self.assert_not_despawned();
        let entity_clone = self.world.spawn_empty().id();

        let mut builder = EntityCloner::build_opt_in(self.world);
        config(&mut builder);
        builder.clone_entity(self.entity, entity_clone);

        self.world.flush();
        self.update_location();
        entity_clone
    }

    /// Clones the specified components of this entity and inserts them into another entity.
    ///
    /// Components can only be cloned if they implement
    /// [`Clone`] or [`Reflect`](bevy_reflect::Reflect).
    ///
    /// # Panics
    ///
    /// - If this entity has been despawned while this `EntityWorldMut` is still alive.
    /// - If the target entity does not exist.
    pub fn clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        self.assert_not_despawned();

        EntityCloner::build_opt_in(self.world)
            .allow::<B>()
            .clone_entity(self.entity, target);

        self.world.flush();
        self.update_location();
        self
    }

    /// Clones the specified components of this entity and inserts them into another entity,
    /// then removes the components from this entity.
    ///
    /// Components can only be cloned if they implement
    /// [`Clone`] or [`Reflect`](bevy_reflect::Reflect).
    ///
    /// # Panics
    ///
    /// - If this entity has been despawned while this `EntityWorldMut` is still alive.
    /// - If the target entity does not exist.
    pub fn move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        self.assert_not_despawned();

        EntityCloner::build_opt_in(self.world)
            .allow::<B>()
            .move_components(true)
            .clone_entity(self.entity, target);

        self.world.flush();
        self.update_location();
        self
    }

    /// Returns the source code location from which this entity has last been spawned.
    pub fn spawned_by(&self) -> MaybeLocation {
        self.world()
            .entities()
            .entity_get_spawned_or_despawned_by(self.entity)
            .map(|location| location.unwrap())
    }

    /// Returns the [`Tick`] at which this entity has last been spawned.
    pub fn spawn_tick(&self) -> Tick {
        self.assert_not_despawned();

        // SAFETY: entity being alive was asserted
        unsafe {
            self.world()
                .entities()
                .entity_get_spawned_or_despawned_unchecked(self.entity)
                .1
        }
    }

    /// Reborrows this entity in a temporary scope.
    /// This is useful for executing a function that requires a `EntityWorldMut`
    /// but you do not want to move out the entity ownership.
    pub fn reborrow_scope<U>(&mut self, f: impl FnOnce(EntityWorldMut) -> U) -> U {
        let Self {
            entity, location, ..
        } = *self;
        self.world_scope(move |world| {
            f(EntityWorldMut {
                world,
                entity,
                location,
            })
        })
    }

    /// Passes the current entity into the given function, and triggers the [`EntityEvent`] returned by that function.
    /// See [`EntityCommands::trigger`] for usage examples
    ///
    /// [`EntityCommands::trigger`]: crate::system::EntityCommands::trigger
    #[track_caller]
    pub fn trigger<'t, E: EntityEvent<Trigger<'t>: Default>>(
        &mut self,
        event_fn: impl FnOnce(Entity) -> E,
    ) -> &mut Self {
        let mut event = (event_fn)(self.entity);
        let caller = MaybeLocation::caller();
        self.world_scope(|world| {
            world.trigger_ref_with_caller(
                &mut event,
                &mut <E::Trigger<'_> as Default>::default(),
                caller,
            );
        });
        self
    }
}

impl<'w> From<EntityWorldMut<'w>> for EntityRef<'w> {
    #[inline]
    fn from(entity: EntityWorldMut<'w>) -> EntityRef<'w> {
        entity.into_readonly()
    }
}

impl<'a> From<&'a EntityWorldMut<'_>> for EntityRef<'a> {
    #[inline]
    fn from(entity: &'a EntityWorldMut<'_>) -> Self {
        entity.as_readonly()
    }
}

impl<'w> From<EntityWorldMut<'w>> for EntityMut<'w> {
    #[inline]
    fn from(entity: EntityWorldMut<'w>) -> Self {
        entity.into_mutable()
    }
}

impl<'a> From<&'a mut EntityWorldMut<'_>> for EntityMut<'a> {
    #[inline]
    fn from(entity: &'a mut EntityWorldMut<'_>) -> Self {
        entity.as_mutable()
    }
}

impl<'a> From<EntityWorldMut<'a>> for FilteredEntityRef<'a, 'static> {
    #[inline]
    fn from(entity: EntityWorldMut<'a>) -> Self {
        entity.into_readonly().into_filtered()
    }
}

impl<'a> From<&'a EntityWorldMut<'_>> for FilteredEntityRef<'a, 'static> {
    #[inline]
    fn from(entity: &'a EntityWorldMut<'_>) -> Self {
        entity.as_readonly().into_filtered()
    }
}

impl<'a> From<EntityWorldMut<'a>> for FilteredEntityMut<'a, 'static> {
    #[inline]
    fn from(entity: EntityWorldMut<'a>) -> Self {
        entity.into_mutable().into_filtered()
    }
}

impl<'a> From<&'a mut EntityWorldMut<'_>> for FilteredEntityMut<'a, 'static> {
    #[inline]
    fn from(entity: &'a mut EntityWorldMut<'_>) -> Self {
        entity.as_mutable().into_filtered()
    }
}

/// Marks a required template as being built while it is alive, to detect cycles.
/// Unmarks it when dropped, including when the template panics.
struct BuildingGuard<'a, 'w> {
    entity: &'a mut EntityWorldMut<'w>,
    building: bool,
}

impl<'a, 'w> BuildingGuard<'a, 'w> {
    fn new(entity: &'a mut EntityWorldMut<'w>, key: Option<(Entity, ComponentId)>) -> Self {
        let building = key.is_some();
        if let Some(key) = key {
            entity.world.required_templates.building.push(key);
        }
        Self { entity, building }
    }
}

impl Drop for BuildingGuard<'_, '_> {
    fn drop(&mut self) {
        if self.building {
            self.entity.world.required_templates.building.pop();
        }
    }
}

/// Inserts a dynamic [`Bundle`] into the entity.
///
/// # Safety
///
/// - [`OwningPtr`] and [`StorageType`] iterators must correspond to the
///   [`BundleInfo`](crate::bundle::BundleInfo) used to construct [`BundleInserter`]
/// - [`Entity`] must correspond to [`EntityLocation`]
unsafe fn insert_dynamic_bundle<
    'a,
    I: Iterator<Item = OwningPtr<'a>>,
    S: Iterator<Item = StorageType>,
>(
    mut bundle_inserter: BundleInserter<'_>,
    entity: Entity,
    location: EntityLocation,
    components: I,
    storage_types: S,
    mode: InsertMode,
    caller: MaybeLocation,
    relationship_hook_insert_mode: RelationshipHookMode,
) -> EntityLocation {
    struct DynamicInsertBundle<'a, I: Iterator<Item = (StorageType, OwningPtr<'a>)>> {
        components: I,
    }

    impl<'a, I: Iterator<Item = (StorageType, OwningPtr<'a>)>> DynamicBundle
        for DynamicInsertBundle<'a, I>
    {
        type Effect = ();
        unsafe fn get_components(
            mut ptr: MovingPtr<'_, Self>,
            func: &mut impl FnMut(StorageType, OwningPtr<'_>),
        ) {
            (&mut ptr.components).for_each(|(t, ptr)| func(t, ptr));
        }

        unsafe fn apply_effect(
            _ptr: MovingPtr<'_, MaybeUninit<Self>>,
            _entity: &mut EntityWorldMut,
        ) {
        }
    }

    let bundle = DynamicInsertBundle {
        components: storage_types.zip(components),
    };

    move_as_ptr!(bundle);

    // SAFETY:
    // - `location` matches `entity`.  and thus must currently exist in the source
    //   archetype for this inserter and its location within the archetype.
    // - The caller must ensure that the iterators and storage types match up with the `BundleInserter`
    // - `DynamicInsertBundle::Effect: NoBundleEffect`
    // - `bundle` is not used or dropped after this point.
    unsafe {
        bundle_inserter.insert(
            entity,
            location,
            bundle,
            mode,
            caller,
            relationship_hook_insert_mode,
        )
    }
}
