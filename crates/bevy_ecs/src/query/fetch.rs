use crate::{
    archetype::{Archetype, ArchetypeComponentId},
    component::{Component, ComponentDescriptor, ComponentTicks, RelationKindId, StorageType},
    entity::Entity,
    query::{Access, FilteredAccess},
    storage::{ComponentSparseSet, Table, Tables},
    world::{Mut, World},
};
use bevy_ecs_macros::all_tuples;
use smallvec::SmallVec;
use std::{
    any::TypeId,
    marker::PhantomData,
    ptr::{self, NonNull},
};

/// Types that can be queried from a [`World`].
///
/// Notable types that implement this trait are `&T` and `&mut T` where `T` implements [`Component`],
/// allowing you to query for components immutably and mutably accordingly.
///
/// See [`Query`](crate::system::Query) for a primer on queries.
///
/// # Basic WorldQueries
///
/// Here is a small list of the most important world queries to know about where `C` stands for a
/// [`Component`] and `WQ` stands for a [`WorldQuery`]:
/// - `&C`: Queries immutably for the component `C`
/// - `&mut C`: Queries mutably for the component `C`
/// - `Option<WQ>`: Queries the inner WorldQuery `WQ` but instead of discarding the entity if the world
///     query fails it returns [`None`]. See [`Query`](crate::system::Query).
/// - `(WQ1, WQ2, ...)`: Queries all contained world queries allowing to query for more than one thing.
///     This is the `And` operator for filters. See [`Or`].
/// - `ChangeTrackers<C>`: See the docs of [`ChangeTrackers`].
/// - [`Entity`]: Using the entity type as a world query will grant access to the entity that is
///     being queried for. See [`Entity`].
///
/// Bevy also offers a few filters like [`Added`](crate::query::Added), [`Changed`](crate::query::Changed),
/// [`With`](crate::query::With), [`Without`](crate::query::Without) and [`Or`].
/// For more information on these consult the item's corresponding documentation.
///
/// [`Or`]: crate::query::Or
pub trait WorldQuery {
    type Fetch: for<'w, 's> Fetch<
        'w,
        's,
        State = Self::State,
        RelationFilter = <Self::State as FetchState>::RelationFilter,
    >;
    type State: FetchState;
}

pub trait Fetch<'w, 's>: Sized {
    type Item;
    type State: FetchState<RelationFilter = Self::RelationFilter>;
    type RelationFilter: Clone + std::hash::Hash + PartialEq + Eq + Default + Send + Sync + 'static;

    /// Creates a new instance of this fetch.
    ///
    /// # Safety
    ///
    /// `state` must have been initialized (via [FetchState::init]) using the same `world` passed in
    /// to this function.
    unsafe fn init(
        world: &World,
        state: &Self::State,
        last_change_tick: u32,
        change_tick: u32,
    ) -> Self;

    /// Returns true if (and only if) every table of every archetype matched by this Fetch contains
    /// all of the matched components. This is used to select a more efficient "table iterator"
    /// for "dense" queries. If this returns true, [`Fetch::set_table`] and [`Fetch::table_fetch`]
    /// will be called for iterators. If this returns false, [`Fetch::set_archetype`] and
    /// [`Fetch::archetype_fetch`] will be called for iterators.
    fn is_dense(&self) -> bool;

    /// Adjusts internal state to account for the next [`Archetype`]. This will always be called on
    /// archetypes that match this [`Fetch`].
    ///
    /// # Safety
    ///
    /// `archetype` and `tables` must be from the [`World`] [`Fetch::init`] was called on. `state` must
    /// be the [Self::State] this was initialized with.
    unsafe fn set_archetype(
        &mut self,
        state: &Self::State,
        relation_filter: &Self::RelationFilter,
        archetype: &Archetype,
        tables: &Tables,
    );

    /// Adjusts internal state to account for the next [`Table`]. This will always be called on tables
    /// that match this [`Fetch`].
    ///
    /// # Safety
    ///
    /// `table` must be from the [`World`] [`Fetch::init`] was called on. `state` must be the
    /// [Self::State] this was initialized with.
    unsafe fn set_table(
        &mut self,
        state: &Self::State,
        relation_filter: &Self::RelationFilter,
        table: &Table,
    );

    /// Fetch [`Self::Item`] for the given `archetype_index` in the current [`Archetype`]. This must
    /// always be called after [`Fetch::set_archetype`] with an `archetype_index` in the range of
    /// the current [`Archetype`]
    ///
    /// # Safety
    /// Must always be called _after_ [`Fetch::set_archetype`]. `archetype_index` must be in the range
    /// of the current archetype
    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item;

    /// Fetch [`Self::Item`] for the given `table_row` in the current [`Table`]. This must always be
    /// called after [`Fetch::set_table`] with a `table_row` in the range of the current [`Table`]
    ///
    /// # Safety
    ///
    /// Must always be called _after_ [`Fetch::set_table`]. `table_row` must be in the range of the
    /// current table
    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item;
}

/// State used to construct a Fetch. This will be cached inside [`QueryState`](crate::query::QueryState),
///  so it is best to move as much data / computation here as possible to reduce the cost of
/// constructing Fetch.
///
/// # Safety
///
/// Implementor must ensure that [`FetchState::update_component_access`] and
/// [`FetchState::update_archetype_component_access`] exactly reflects the results of
/// [`FetchState::matches_archetype`], [`FetchState::matches_table`], [`Fetch::archetype_fetch`], and
/// [`Fetch::table_fetch`].
pub unsafe trait FetchState: Send + Sync + Sized {
    type RelationFilter: Clone + std::hash::Hash + PartialEq + Eq + Default + Send + Sync + 'static;

    fn init(world: &mut World) -> Self;
    fn update_component_access(&self, access: &mut FilteredAccess<RelationKindId>);
    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    );
    fn matches_archetype(
        &self,
        archetype: &Archetype,
        relation_filter: &Self::RelationFilter,
    ) -> bool;
    fn matches_table(&self, table: &Table, relation_filter: &Self::RelationFilter) -> bool;
}

/// A fetch that is read only. This must only be implemented for read-only fetches.
pub unsafe trait ReadOnlyFetch {}

impl WorldQuery for Entity {
    type Fetch = EntityFetch;
    type State = EntityState;
}

/// The [`Fetch`] of [`Entity`].
pub struct EntityFetch {
    entities: *const Entity,
}

/// SAFETY: access is read only
unsafe impl ReadOnlyFetch for EntityFetch {}

/// The [`FetchState`] of [`Entity`].
pub struct EntityState;

// SAFETY: no component or archetype access
unsafe impl FetchState for EntityState {
    type RelationFilter = ();

    fn init(_world: &mut World) -> Self {
        Self
    }

    fn update_component_access(&self, _access: &mut FilteredAccess<RelationKindId>) {}

    fn update_archetype_component_access(
        &self,
        _archetype: &Archetype,
        _access: &mut Access<ArchetypeComponentId>,
    ) {
    }

    #[inline]
    fn matches_archetype(
        &self,
        _archetype: &Archetype,
        _relation_filter: &Self::RelationFilter,
    ) -> bool {
        true
    }

    #[inline]
    fn matches_table(&self, _table: &Table, _relation_filter: &Self::RelationFilter) -> bool {
        true
    }
}

impl<'w, 's> Fetch<'w, 's> for EntityFetch {
    type Item = Entity;
    type State = EntityState;
    type RelationFilter = ();

    #[inline]
    fn is_dense(&self) -> bool {
        true
    }

    unsafe fn init(
        _world: &World,
        _state: &Self::State,
        _last_change_tick: u32,
        _change_tick: u32,
    ) -> Self {
        Self {
            entities: std::ptr::null::<Entity>(),
        }
    }

    #[inline]
    unsafe fn set_archetype(
        &mut self,
        _state: &Self::State,
        _relation_filter: &Self::RelationFilter,
        archetype: &Archetype,
        _tables: &Tables,
    ) {
        self.entities = archetype.entities().as_ptr();
    }

    #[inline]
    unsafe fn set_table(
        &mut self,
        _state: &Self::State,
        _relation_filter: &Self::RelationFilter,
        table: &Table,
    ) {
        self.entities = table.entities().as_ptr();
    }

    #[inline]
    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item {
        *self.entities.add(table_row)
    }

    #[inline]
    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item {
        *self.entities.add(archetype_index)
    }
}

impl<T: Component> WorldQuery for &T {
    type Fetch = ReadFetch<T>;
    type State = ReadState<T>;
}

/// The [`FetchState`] of `&T`.
pub struct ReadState<T> {
    relation_kind_id: RelationKindId,
    relation_target: Option<Entity>,
    storage_type: StorageType,
    marker: PhantomData<T>,
}

// SAFETY: component access and archetype component access are properly updated to reflect that T is
// read
unsafe impl<T: Component> FetchState for ReadState<T> {
    type RelationFilter = ();

    fn init(world: &mut World) -> Self {
        let kind_info = world.relationships.get_component_kind_or_insert(
            TypeId::of::<T>(),
            ComponentDescriptor::from_generic::<T>(StorageType::Table),
        );
        ReadState {
            relation_kind_id: kind_info.id(),
            relation_target: None,
            storage_type: kind_info.data_layout().storage_type(),
            marker: PhantomData,
        }
    }

    fn update_component_access(&self, access: &mut FilteredAccess<RelationKindId>) {
        if access.access().has_write(self.component_id) {
            panic!("&{} conflicts with a previous access in this query. Shared access cannot coincide with exclusive access.",
                std::any::type_name::<T>());
        }
        access.add_read(self.component_id)
    }

    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        if let Some(archetype_component_id) =
            archetype.get_archetype_component_id(self.relation_kind_id, self.relation_target)
        {
            access.add_read(archetype_component_id);
        }
    }

    fn matches_archetype(
        &self,
        archetype: &Archetype,
        _relation_filter: &Self::RelationFilter,
    ) -> bool {
        archetype.contains(self.relation_kind_id, self.relation_target)
    }

    fn matches_table(&self, table: &Table, _relation_filter: &Self::RelationFilter) -> bool {
        table.has_column(self.relation_kind_id, self.relation_target)
    }
}

/// The [`Fetch`] of `&T`.
pub struct ReadFetch<T> {
    storage_type: StorageType,
    table_components: NonNull<T>,
    entity_table_rows: *const usize,
    entities: *const Entity,
    sparse_set: *const ComponentSparseSet,
}

/// SAFETY: access is read only
unsafe impl<T> ReadOnlyFetch for ReadFetch<T> {}

impl<'w, 's, T: Component> Fetch<'w, 's> for ReadFetch<T> {
    type Item = &'w T;
    type State = ReadState<T>;
    type RelationFilter = ();

    #[inline]
    fn is_dense(&self) -> bool {
        match self.storage_type {
            StorageType::Table => true,
            StorageType::SparseSet => false,
        }
    }

    unsafe fn init(
        world: &World,
        state: &Self::State,
        _last_change_tick: u32,
        _change_tick: u32,
    ) -> Self {
        let mut value = Self {
            storage_type: state.storage_type,
            table_components: NonNull::dangling(),
            entities: ptr::null::<Entity>(),
            entity_table_rows: ptr::null::<usize>(),
            sparse_set: ptr::null::<ComponentSparseSet>(),
        };
        if state.storage_type == StorageType::SparseSet {
            value.sparse_set = world
                .storages()
                .sparse_sets
                .get(state.relation_kind_id, state.relation_target)
                .unwrap();
        }
        value
    }

    #[inline]
    unsafe fn set_archetype(
        &mut self,
        state: &Self::State,
        _relation_filter: &Self::RelationFilter,
        archetype: &Archetype,
        tables: &Tables,
    ) {
        match state.storage_type {
            StorageType::Table => {
                self.entity_table_rows = archetype.entity_table_rows().as_ptr();
                let column = tables[archetype.table_id()]
                    .get_column(state.relation_kind_id, state.relation_target)
                    .unwrap();
                self.table_components = column.get_ptr().cast::<T>();
            }
            StorageType::SparseSet => self.entities = archetype.entities().as_ptr(),
        }
    }

    #[inline]
    unsafe fn set_table(
        &mut self,
        state: &Self::State,
        _relation_filter: &Self::RelationFilter,
        table: &Table,
    ) {
        self.table_components = table
            .get_column(state.relation_kind_id, state.relation_target)
            .unwrap()
            .get_ptr()
            .cast::<T>();
    }

    #[inline]
    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item {
        match self.storage_type {
            StorageType::Table => {
                let table_row = *self.entity_table_rows.add(archetype_index);
                &*self.table_components.as_ptr().add(table_row)
            }
            StorageType::SparseSet => {
                let entity = *self.entities.add(archetype_index);
                &*(*self.sparse_set).get(entity).unwrap().cast::<T>()
            }
        }
    }

    #[inline]
    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item {
        &*self.table_components.as_ptr().add(table_row)
    }
}

impl<T: Component> WorldQuery for &mut T {
    type Fetch = WriteFetch<T>;
    type State = WriteState<T>;
}

/// The [`Fetch`] of `&mut T`.
pub struct WriteFetch<T> {
    storage_type: StorageType,
    table_components: NonNull<T>,
    table_ticks: *mut ComponentTicks,
    entities: *const Entity,
    entity_table_rows: *const usize,
    sparse_set: *const ComponentSparseSet,
    last_change_tick: u32,
    change_tick: u32,
}

/// The [`FetchState`] of `&mut T`.
pub struct WriteState<T> {
    relation_kind_id: RelationKindId,
    relation_target: Option<Entity>,
    storage_type: StorageType,
    marker: PhantomData<T>,
}

// SAFETY: component access and archetype component access are properly updated to reflect that T is
// written
unsafe impl<T: Component> FetchState for WriteState<T> {
    type RelationFilter = ();

    fn init(world: &mut World) -> Self {
        let kind_info = world.relationships.get_component_kind_or_insert(
            TypeId::of::<T>(),
            ComponentDescriptor::from_generic::<T>(StorageType::Table),
        );
        WriteState {
            relation_kind_id: kind_info.id(),
            relation_target: None,
            storage_type: kind_info.data_layout().storage_type(),
            marker: PhantomData,
        }
    }

    fn update_component_access(&self, access: &mut FilteredAccess<RelationKindId>) {
        if access.access().has_read(self.relation_kind_id) {
            panic!("&mut {} conflicts with a previous access in this query. Mutable component access must be unique.",
                std::any::type_name::<T>());
        }
        access.add_write(self.relation_kind_id);
    }

    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        if let Some(archetype_component_id) =
            archetype.get_archetype_component_id(self.relation_kind_id, self.relation_target)
        {
            access.add_write(archetype_component_id);
        }
    }

    fn matches_archetype(
        &self,
        archetype: &Archetype,
        _relation_filter: &Self::RelationFilter,
    ) -> bool {
        archetype.contains(self.relation_kind_id, self.relation_target)
    }

    fn matches_table(&self, table: &Table, _relation_filter: &Self::RelationFilter) -> bool {
        table.has_column(self.relation_kind_id, self.relation_target)
    }
}

impl<'w, 's, T: Component> Fetch<'w, 's> for WriteFetch<T> {
    type Item = Mut<'w, T>;
    type State = WriteState<T>;
    type RelationFilter = ();

    #[inline]
    fn is_dense(&self) -> bool {
        match self.storage_type {
            StorageType::Table => true,
            StorageType::SparseSet => false,
        }
    }

    unsafe fn init(
        world: &World,
        state: &Self::State,
        last_change_tick: u32,
        change_tick: u32,
    ) -> Self {
        let mut value = Self {
            storage_type: state.storage_type,
            table_components: NonNull::dangling(),
            entities: ptr::null::<Entity>(),
            entity_table_rows: ptr::null::<usize>(),
            sparse_set: ptr::null::<ComponentSparseSet>(),
            table_ticks: ptr::null_mut::<ComponentTicks>(),
            last_change_tick,
            change_tick,
        };
        if state.storage_type == StorageType::SparseSet {
            value.sparse_set = world
                .storages()
                .sparse_sets
                .get(state.relation_kind_id, state.relation_target)
                .unwrap();
        }
        value
    }

    #[inline]
    unsafe fn set_archetype(
        &mut self,
        state: &Self::State,
        _relation_filter: &Self::RelationFilter,
        archetype: &Archetype,
        tables: &Tables,
    ) {
        match state.storage_type {
            StorageType::Table => {
                self.entity_table_rows = archetype.entity_table_rows().as_ptr();
                let column = tables[archetype.table_id()]
                    .get_column(state.relation_kind_id, state.relation_target)
                    .unwrap();
                self.table_components = column.get_ptr().cast::<T>();
                self.table_ticks = column.get_ticks_mut_ptr();
            }
            StorageType::SparseSet => self.entities = archetype.entities().as_ptr(),
        }
    }

    #[inline]
    unsafe fn set_table(
        &mut self,
        state: &Self::State,
        _relation_filter: &Self::RelationFilter,
        table: &Table,
    ) {
        let column = table
            .get_column(state.relation_kind_id, state.relation_target)
            .unwrap();
        self.table_components = column.get_ptr().cast::<T>();
        self.table_ticks = column.get_ticks_mut_ptr();
    }

    #[inline]
    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item {
        match self.storage_type {
            StorageType::Table => {
                let table_row = *self.entity_table_rows.add(archetype_index);
                Mut {
                    value: &mut *self.table_components.as_ptr().add(table_row),
                    component_ticks: &mut *self.table_ticks.add(table_row),
                    change_tick: self.change_tick,
                    last_change_tick: self.last_change_tick,
                }
            }
            StorageType::SparseSet => {
                let entity = *self.entities.add(archetype_index);
                let (component, component_ticks) =
                    (*self.sparse_set).get_with_ticks(entity).unwrap();
                Mut {
                    value: &mut *component.cast::<T>(),
                    component_ticks: &mut *component_ticks,
                    change_tick: self.change_tick,
                    last_change_tick: self.last_change_tick,
                }
            }
        }
    }

    #[inline]
    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item {
        Mut {
            value: &mut *self.table_components.as_ptr().add(table_row),
            component_ticks: &mut *self.table_ticks.add(table_row),
            change_tick: self.change_tick,
            last_change_tick: self.last_change_tick,
        }
    }
}

pub struct Relation<T: Component>(std::marker::PhantomData<T>, [u8]);

impl<T: Component> WorldQuery for &Relation<T> {
    type Fetch = ReadRelationFetch<T>;
    type State = ReadRelationState<T>;
}

pub struct ReadRelationState<T> {
    p: PhantomData<T>,
    relation_kind: RelationKindId,
    storage_type: StorageType,
}

pub struct ReadRelationFetch<T> {
    storage_type: StorageType,
    p: PhantomData<T>,
}

unsafe impl<T: Component> ReadOnlyFetch for ReadRelationFetch<T> {}

unsafe impl<T: Component> FetchState for ReadRelationState<T> {
    type RelationFilter = smallvec::SmallVec<[Entity; 4]>;

    fn init(world: &mut World) -> Self {
        let kind_info = world.relationships.get_component_kind_or_insert(
            TypeId::of::<T>(),
            ComponentDescriptor::from_generic::<T>(StorageType::Table),
        );
        Self {
            p: PhantomData,
            relation_kind: kind_info.id(),
            storage_type: kind_info.data_layout().storage_type(),
        }
    }

    fn update_component_access(&self, access: &mut FilteredAccess<RelationKindId>) {
        access.add_read(self.relation_kind);
    }

    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        if self.matches_archetype(archetype, &Default::default()) {
            let targets = archetype.components.get(self.relation_kind).unwrap();
            if let Some(id) = &targets.0 {
                access.add_read(id.archetype_component_id);
            }
            for id in targets.1.values() {
                access.add_read(id.archetype_component_id);
            }
        }
    }

    fn matches_archetype(
        &self,
        archetype: &Archetype,
        relation_filter: &SmallVec<[Entity; 4]>,
    ) -> bool {
        if archetype.components.get(self.relation_kind).is_none() {
            return false;
        }
        relation_filter
            .iter()
            .all(|target| archetype.contains(self.relation_kind, Some(*target)))
    }

    fn matches_table(&self, table: &Table, relation_filter: &SmallVec<[Entity; 4]>) -> bool {
        if table.columns.get(self.relation_kind).is_none() {
            return false;
        }
        relation_filter
            .iter()
            .all(|target| table.has_column(self.relation_kind, Some(*target)))
    }
}

pub struct RelationAccess<'w, 's, T: Component> {
    p: PhantomData<(&'w T, &'s T)>,
}

impl<'w, 's, T: Component> Fetch<'w, 's> for ReadRelationFetch<T> {
    type Item = RelationAccess<'w, 's, T>;
    type State = ReadRelationState<T>;
    type RelationFilter = smallvec::SmallVec<[Entity; 4]>;

    unsafe fn init(
        world: &World,
        state: &Self::State,
        _last_change_tick: u32,
        _change_tick: u32,
    ) -> Self {
        let storage_type = world
            .components()
            .get_relation_kind(state.relation_kind)
            .unwrap()
            .data_layout()
            .storage_type();

        Self {
            storage_type,
            p: PhantomData,
        }
    }

    fn is_dense(&self) -> bool {
        match self.storage_type {
            StorageType::Table => true,
            StorageType::SparseSet => false,
        }
    }

    unsafe fn set_archetype(
        &mut self,
        state: &Self::State,
        relation_filter: &Self::RelationFilter,
        archetype: &Archetype,
        tables: &Tables,
    ) {
        ()
    }

    unsafe fn set_table(
        &mut self,
        state: &Self::State,
        relation_filter: &Self::RelationFilter,
        table: &Table,
    ) {
        ()
    }

    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item {
        todo!()
    }

    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item {
        todo!()
    }
}

impl<T: WorldQuery> WorldQuery for Option<T> {
    type Fetch = OptionFetch<T::Fetch>;
    type State = OptionState<T::State>;
}

/// The [`Fetch`] of `Option<T>`.
pub struct OptionFetch<T> {
    fetch: T,
    matches: bool,
}

/// SAFETY: OptionFetch is read only because T is read only
unsafe impl<T: ReadOnlyFetch> ReadOnlyFetch for OptionFetch<T> {}

/// The [`FetchState`] of `Option<T>`.
pub struct OptionState<T: FetchState> {
    state: T,
}

// SAFETY: component access and archetype component access are properly updated according to the
// internal Fetch
unsafe impl<T: FetchState> FetchState for OptionState<T> {
    type RelationFilter = T::RelationFilter;

    fn init(world: &mut World) -> Self {
        Self {
            state: T::init(world),
        }
    }

    fn update_component_access(&self, access: &mut FilteredAccess<RelationKindId>) {
        self.state.update_component_access(access);
    }

    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        // FIXME(Relationships) is default right..?
        if self.state.matches_archetype(archetype, &Default::default()) {
            self.state
                .update_archetype_component_access(archetype, access)
        }
    }

    fn matches_archetype(
        &self,
        _archetype: &Archetype,
        _relation_filter: &Self::RelationFilter,
    ) -> bool {
        true
    }

    fn matches_table(&self, _table: &Table, _relation_filter: &Self::RelationFilter) -> bool {
        true
    }
}

impl<'w, 's, T: Fetch<'w, 's>> Fetch<'w, 's> for OptionFetch<T> {
    type Item = Option<T::Item>;
    type State = OptionState<T::State>;
    type RelationFilter = T::RelationFilter;

    #[inline]
    fn is_dense(&self) -> bool {
        self.fetch.is_dense()
    }

    unsafe fn init(
        world: &World,
        state: &Self::State,
        last_change_tick: u32,
        change_tick: u32,
    ) -> Self {
        Self {
            fetch: T::init(world, &state.state, last_change_tick, change_tick),
            matches: false,
        }
    }

    #[inline]
    unsafe fn set_archetype(
        &mut self,
        state: &Self::State,
        relation_filter: &Self::RelationFilter,
        archetype: &Archetype,
        tables: &Tables,
    ) {
        // FIXME(Relationships) I don't get why we need to do this matching here.
        // why do we call set_archetype with archetypes that potentially dont match..?
        self.matches = state.state.matches_archetype(archetype, relation_filter);
        if self.matches {
            self.fetch
                .set_archetype(&state.state, relation_filter, archetype, tables);
        }
    }

    #[inline]
    unsafe fn set_table(
        &mut self,
        state: &Self::State,
        relation_filter: &Self::RelationFilter,
        table: &Table,
    ) {
        self.matches = state.state.matches_table(table, relation_filter);
        if self.matches {
            self.fetch.set_table(&state.state, relation_filter, table);
        }
    }

    #[inline]
    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item {
        if self.matches {
            Some(self.fetch.archetype_fetch(archetype_index))
        } else {
            None
        }
    }

    #[inline]
    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item {
        if self.matches {
            Some(self.fetch.table_fetch(table_row))
        } else {
            None
        }
    }
}

/// [`WorldQuery`] that tracks changes and additions for component `T`.
///
/// Wraps a [`Component`] to track whether the component changed for the corresponding entities in
/// a query since the last time the system that includes these queries ran.
///
/// If you only care about entities that changed or that got added use the
/// [`Changed`](crate::query::Changed) and [`Added`](crate::query::Added) filters instead.
///
/// # Examples
///
/// ```
/// # use bevy_ecs::system::Query;
/// # use bevy_ecs::query::ChangeTrackers;
/// # use bevy_ecs::system::IntoSystem;
/// #
/// # #[derive(Debug)]
/// # struct Name {};
/// # struct Transform {};
/// #
/// fn print_moving_objects_system(query: Query<(&Name, ChangeTrackers<Transform>)>) {
///     for (name, tracker) in query.iter() {
///         if tracker.is_changed() {
///             println!("Entity moved: {:?}", name);
///         } else {
///             println!("Entity stood still: {:?}", name);
///         }
///     }
/// }
/// # print_moving_objects_system.system();
/// ```
#[derive(Clone)]
pub struct ChangeTrackers<T: Component> {
    pub(crate) component_ticks: ComponentTicks,
    pub(crate) last_change_tick: u32,
    pub(crate) change_tick: u32,
    marker: PhantomData<T>,
}

impl<T: Component> std::fmt::Debug for ChangeTrackers<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChangeTrackers")
            .field("component_ticks", &self.component_ticks)
            .field("last_change_tick", &self.last_change_tick)
            .field("change_tick", &self.change_tick)
            .finish()
    }
}

impl<T: Component> ChangeTrackers<T> {
    /// Returns true if this component has been added since the last execution of this system.
    pub fn is_added(&self) -> bool {
        self.component_ticks
            .is_added(self.last_change_tick, self.change_tick)
    }

    /// Returns true if this component has been changed since the last execution of this system.
    pub fn is_changed(&self) -> bool {
        self.component_ticks
            .is_changed(self.last_change_tick, self.change_tick)
    }
}

impl<T: Component> WorldQuery for ChangeTrackers<T> {
    type Fetch = ChangeTrackersFetch<T>;
    type State = ChangeTrackersState<T>;
}

/// The [`FetchState`] of [`ChangeTrackers`].
pub struct ChangeTrackersState<T> {
    relation_kind_id: RelationKindId,
    relation_target: Option<Entity>,
    storage_type: StorageType,
    marker: PhantomData<T>,
}

// SAFETY: component access and archetype component access are properly updated to reflect that T is
// read
unsafe impl<T: Component> FetchState for ChangeTrackersState<T> {
    type RelationFilter = ();

    fn init(world: &mut World) -> Self {
        let kind_info = world.relationships.get_component_kind_or_insert(
            TypeId::of::<T>(),
            ComponentDescriptor::from_generic::<T>(StorageType::Table),
        );
        Self {
            relation_kind_id: kind_info.id(),
            relation_target: None,
            storage_type: kind_info.data_layout().storage_type(),
            marker: PhantomData,
        }
    }

    fn update_component_access(&self, access: &mut FilteredAccess<RelationKindId>) {
        if access.access().has_write(self.component_id) {
            panic!("ChangeTrackers<{}> conflicts with a previous access in this query. Shared access cannot coincide with exclusive access.",
                std::any::type_name::<T>());
        }
        access.add_read(self.component_id)
    }

    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        if let Some(archetype_component_id) =
            archetype.get_archetype_component_id(self.relation_kind_id, self.relation_target)
        {
            access.add_read(archetype_component_id);
        }
    }

    fn matches_archetype(
        &self,
        archetype: &Archetype,
        _relation_filter: &Self::RelationFilter,
    ) -> bool {
        archetype.contains(self.relation_kind_id, self.relation_target)
    }

    fn matches_table(&self, table: &Table, _relation_filter: &Self::RelationFilter) -> bool {
        table.has_column(self.relation_kind_id, self.relation_target)
    }
}

/// The [`Fetch`] of [`ChangeTrackers`].
pub struct ChangeTrackersFetch<T> {
    storage_type: StorageType,
    table_ticks: *const ComponentTicks,
    entity_table_rows: *const usize,
    entities: *const Entity,
    sparse_set: *const ComponentSparseSet,
    marker: PhantomData<T>,
    last_change_tick: u32,
    change_tick: u32,
}

/// SAFETY: access is read only
unsafe impl<T> ReadOnlyFetch for ChangeTrackersFetch<T> {}

impl<'w, 's, T: Component> Fetch<'w, 's> for ChangeTrackersFetch<T> {
    type Item = ChangeTrackers<T>;
    type State = ChangeTrackersState<T>;
    type RelationFilter = ();

    #[inline]
    fn is_dense(&self) -> bool {
        match self.storage_type {
            StorageType::Table => true,
            StorageType::SparseSet => false,
        }
    }

    unsafe fn init(
        world: &World,
        state: &Self::State,
        last_change_tick: u32,
        change_tick: u32,
    ) -> Self {
        let mut value = Self {
            storage_type: state.storage_type,
            table_ticks: ptr::null::<ComponentTicks>(),
            entities: ptr::null::<Entity>(),
            entity_table_rows: ptr::null::<usize>(),
            sparse_set: ptr::null::<ComponentSparseSet>(),
            marker: PhantomData,
            last_change_tick,
            change_tick,
        };
        if state.storage_type == StorageType::SparseSet {
            value.sparse_set = world
                .storages()
                .sparse_sets
                .get(state.relation_kind_id, state.relation_target)
                .unwrap();
        }
        value
    }

    #[inline]
    unsafe fn set_archetype(
        &mut self,
        state: &Self::State,
        _relation_filter: &Self::RelationFilter,
        archetype: &Archetype,
        tables: &Tables,
    ) {
        match state.storage_type {
            StorageType::Table => {
                self.entity_table_rows = archetype.entity_table_rows().as_ptr();
                let column = tables[archetype.table_id()]
                    .get_column(state.relation_kind_id, state.relation_target)
                    .unwrap();
                self.table_ticks = column.get_ticks_mut_ptr().cast::<ComponentTicks>();
            }
            StorageType::SparseSet => self.entities = archetype.entities().as_ptr(),
        }
    }

    #[inline]
    unsafe fn set_table(
        &mut self,
        state: &Self::State,
        _relation_filter: &Self::RelationFilter,
        table: &Table,
    ) {
        self.table_ticks = table
            .get_column(state.relation_kind_id, state.relation_target)
            .unwrap()
            .get_ticks_mut_ptr()
            .cast::<ComponentTicks>();
    }

    #[inline]
    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item {
        match self.storage_type {
            StorageType::Table => {
                let table_row = *self.entity_table_rows.add(archetype_index);
                ChangeTrackers {
                    component_ticks: *self.table_ticks.add(table_row),
                    marker: PhantomData,
                    last_change_tick: self.last_change_tick,
                    change_tick: self.change_tick,
                }
            }
            StorageType::SparseSet => {
                let entity = *self.entities.add(archetype_index);
                ChangeTrackers {
                    component_ticks: *(*self.sparse_set).get_ticks(entity).unwrap(),
                    marker: PhantomData,
                    last_change_tick: self.last_change_tick,
                    change_tick: self.change_tick,
                }
            }
        }
    }

    #[inline]
    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item {
        ChangeTrackers {
            component_ticks: *self.table_ticks.add(table_row),
            marker: PhantomData,
            last_change_tick: self.last_change_tick,
            change_tick: self.change_tick,
        }
    }
}

macro_rules! impl_tuple_fetch {
    ($(($name: ident, $state: ident, $relation_filter: ident)),*) => {
        #[allow(non_snake_case)]
        impl<'w, 's, $($name: Fetch<'w, 's>),*> Fetch<'w, 's> for ($($name,)*) {
            type Item = ($($name::Item,)*);
            type State = ($($name::State,)*);
            type RelationFilter = ($($name::RelationFilter,)*);

            unsafe fn init(_world: &World, state: &Self::State, _last_change_tick: u32, _change_tick: u32) -> Self {
                let ($($name,)*) = state;
                ($($name::init(_world, $name, _last_change_tick, _change_tick),)*)
            }


            #[inline]
            fn is_dense(&self) -> bool {
                let ($($name,)*) = self;
                true $(&& $name.is_dense())*
            }

            #[inline]
            unsafe fn set_archetype(&mut self, _state: &Self::State, relation_filter: &Self::RelationFilter, _archetype: &Archetype, _tables: &Tables) {
                let ($($name,)*) = self;
                let ($($state,)*) = _state;
                let ($($relation_filter,)*) = relation_filter;
                $($name.set_archetype($state, $relation_filter, _archetype, _tables);)*
            }

            #[inline]
            unsafe fn set_table(&mut self, _state: &Self::State, _relation_filter: &Self::RelationFilter, _table: &Table) {
                let ($($name,)*) = self;
                let ($($state,)*) = _state;
                let ($($relation_filter,)*) = _relation_filter;
                $($name.set_table($state, $relation_filter, _table);)*
            }

            #[inline]
            unsafe fn table_fetch(&mut self, _table_row: usize) -> Self::Item {
                let ($($name,)*) = self;
                ($($name.table_fetch(_table_row),)*)
            }

            #[inline]
            unsafe fn archetype_fetch(&mut self, _archetype_index: usize) -> Self::Item {
                let ($($name,)*) = self;
                ($($name.archetype_fetch(_archetype_index),)*)
            }
        }

        // SAFETY: update_component_access and update_archetype_component_access are called for each item in the tuple
        #[allow(non_snake_case)]
        unsafe impl<$($name: FetchState),*> FetchState for ($($name,)*) {
            type RelationFilter = ($($name::RelationFilter,)*);

            fn init(_world: &mut World) -> Self {
                ($($name::init(_world),)*)
            }

            fn update_component_access(&self, _access: &mut FilteredAccess<RelationKindId>) {
                let ($($name,)*) = self;
                $($name.update_component_access(_access);)*
            }

            fn update_archetype_component_access(&self, _archetype: &Archetype, _access: &mut Access<ArchetypeComponentId>) {
                let ($($name,)*) = self;
                $($name.update_archetype_component_access(_archetype, _access);)*
            }

            fn matches_archetype(&self, _archetype: &Archetype, _relation_filter: &Self::RelationFilter) -> bool {
                let ($($name,)*) = self;
                let ($($relation_filter,)*) = _relation_filter;
                true $(&& $name.matches_archetype(_archetype, $relation_filter))*
            }

            fn matches_table(&self, _table: &Table, _relation_filter: &Self::RelationFilter) -> bool {
                let ($($name,)*) = self;
                let ($($relation_filter,)*) = _relation_filter;
                true $(&& $name.matches_table(_table, $relation_filter))*
            }
        }

        impl<$($name: WorldQuery),*> WorldQuery for ($($name,)*) {
            type Fetch = ($($name::Fetch,)*);
            type State = ($($name::State,)*);
        }

        /// SAFETY: each item in the tuple is read only
        unsafe impl<$($name: ReadOnlyFetch),*> ReadOnlyFetch for ($($name,)*) {}

    };
}

all_tuples!(impl_tuple_fetch, 0, 11, F, S, R);
