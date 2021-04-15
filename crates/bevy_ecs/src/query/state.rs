use std::collections::HashMap;

use crate::{
    archetype::{Archetype, ArchetypeComponentId, ArchetypeGeneration, ArchetypeId},
    component::RelationKindId,
    entity::Entity,
    query::{
        Access, Fetch, FetchState, FilterFetch, FilteredAccess, QueryIter, ReadOnlyFetch,
        WorldQuery,
    },
    storage::TableId,
    world::{World, WorldId},
};
use bevy_tasks::TaskPool;
use fixedbitset::FixedBitSet;
use thiserror::Error;

use super::QueryRelationFilter;

pub struct QueryAccessCache {
    pub(crate) archetype_generation: ArchetypeGeneration,
    pub(crate) matched_tables: FixedBitSet,
    pub(crate) matched_archetypes: FixedBitSet,
    // NOTE: we maintain both a TableId bitset and a vec because iterating the vec is faster
    pub(crate) matched_table_ids: Vec<TableId>,
    // NOTE: we maintain both a ArchetypeId bitset and a vec because iterating the vec is faster
    pub(crate) matched_archetype_ids: Vec<ArchetypeId>,
}

pub struct QueryState<Q: WorldQuery, F: WorldQuery = ()>
where
    F::Fetch: FilterFetch,
{
    world_id: WorldId,
    pub(crate) archetype_component_access: Access<ArchetypeComponentId>,
    pub(crate) component_access: FilteredAccess<RelationKindId>,

    // FIXME(Relationships) We need to clear this on `Query` drop impl so that filters dont
    // persist across system executions
    pub(crate) current_relation_filter: QueryRelationFilter<Q, F>,
    pub(crate) relation_filter_accesses: HashMap<QueryRelationFilter<Q, F>, QueryAccessCache>,

    pub(crate) fetch_state: Q::State,
    pub(crate) filter_state: F::State,
}

impl<Q: WorldQuery, F: WorldQuery> QueryState<Q, F>
where
    F::Fetch: FilterFetch,
{
    pub fn new(world: &mut World) -> Self {
        let fetch_state = <Q::State as FetchState>::init(world);
        let filter_state = <F::State as FetchState>::init(world);

        let mut component_access = FilteredAccess::default();
        fetch_state.update_component_access(&mut component_access);

        // Use a temporary empty FilteredAccess for filters. This prevents them from conflicting with the
        // main Query's `fetch_state` access. Filters are allowed to conflict with the main query fetch
        // because they are evaluated *before* a specific reference is constructed.
        let mut filter_component_access = FilteredAccess::default();
        filter_state.update_component_access(&mut filter_component_access);

        // Merge the temporary filter access with the main access. This ensures that filter access is
        // properly considered in a global "cross-query" context (both within systems and across systems).
        component_access.extend(&filter_component_access);

        let mut state = Self {
            world_id: world.id(),
            fetch_state,
            filter_state,
            component_access,

            current_relation_filter: Default::default(),
            relation_filter_accesses: HashMap::new(),

            archetype_component_access: Default::default(),
        };
        state.set_relation_filter(world, QueryRelationFilter::default());
        state.validate_world_and_update_archetypes(world);
        state
    }

    pub fn current_query_access_cache(&self) -> &QueryAccessCache {
        self.relation_filter_accesses
            .get(&self.current_relation_filter)
            .unwrap()
    }

    pub fn set_relation_filter(
        &mut self,
        world: &World,
        relation_filter: QueryRelationFilter<Q, F>,
    ) {
        self.current_relation_filter = relation_filter.clone();
        self.relation_filter_accesses
            .entry(relation_filter)
            .or_insert(QueryAccessCache {
                archetype_generation: ArchetypeGeneration::new(usize::MAX),
                matched_table_ids: Vec::new(),
                matched_archetype_ids: Vec::new(),
                matched_tables: Default::default(),
                matched_archetypes: Default::default(),
            });
        self.validate_world_and_update_archetypes(world);
    }

    pub fn validate_world_and_update_archetypes(&mut self, world: &World) {
        if world.id() != self.world_id {
            panic!("Attempted to use {} with a mismatched World. QueryStates can only be used with the World they were created from.",
                std::any::type_name::<Self>());
        }
        let archetypes = world.archetypes();

        for (relation_filter, cache) in self.relation_filter_accesses.iter_mut() {
            let old_generation = cache.archetype_generation;
            let archetype_index_range = if old_generation == archetypes.generation() {
                0..0
            } else {
                cache.archetype_generation = archetypes.generation();
                if old_generation.value() == usize::MAX {
                    0..archetypes.len()
                } else {
                    old_generation.value()..archetypes.len()
                }
            };
            for archetype_index in archetype_index_range {
                let archetype = &archetypes[ArchetypeId::new(archetype_index)];
                Self::new_archetype(
                    &self.fetch_state,
                    &self.filter_state,
                    &mut self.archetype_component_access,
                    &*relation_filter,
                    cache,
                    archetype,
                );
            }
        }
    }

    pub fn new_archetype(
        fetch_state: &Q::State,
        filter_state: &F::State,
        access: &mut Access<ArchetypeComponentId>,
        relation_filter: &QueryRelationFilter<Q, F>,
        cache: &mut QueryAccessCache,
        archetype: &Archetype,
    ) {
        if fetch_state.matches_archetype(archetype, &relation_filter.0)
            && filter_state.matches_archetype(archetype, &relation_filter.1)
        {
            fetch_state.update_archetype_component_access(archetype, access);
            filter_state.update_archetype_component_access(archetype, access);

            let archetype_index = archetype.id().index();
            if !cache.matched_archetypes.contains(archetype_index) {
                cache.matched_archetypes.grow(archetype.id().index() + 1);
                cache.matched_archetypes.set(archetype.id().index(), true);
                cache.matched_archetype_ids.push(archetype.id());
            }
            let table_index = archetype.table_id().index();
            if !cache.matched_tables.contains(table_index) {
                cache.matched_tables.grow(table_index + 1);
                cache.matched_tables.set(table_index, true);
                cache.matched_table_ids.push(archetype.table_id());
            }
        }
    }

    #[inline]
    pub fn get<'w>(
        &mut self,
        world: &'w World,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch<'w, '_>>::Item, QueryEntityError>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFETY: query is read only
        unsafe { self.get_unchecked(world, entity) }
    }

    #[inline]
    pub fn get_mut<'w>(
        &mut self,
        world: &'w mut World,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch<'w, '_>>::Item, QueryEntityError> {
        // SAFETY: query has unique world access
        unsafe { self.get_unchecked(world, entity) }
    }

    /// # Safety
    ///
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    #[inline]
    pub unsafe fn get_unchecked<'w>(
        &mut self,
        world: &'w World,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch<'w, '_>>::Item, QueryEntityError> {
        self.validate_world_and_update_archetypes(world);
        self.get_unchecked_manual(
            world,
            entity,
            world.last_change_tick(),
            world.read_change_tick(),
        )
    }

    /// # Safety
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    pub unsafe fn get_unchecked_manual<'w>(
        &self,
        world: &'w World,
        entity: Entity,
        last_change_tick: u32,
        change_tick: u32,
    ) -> Result<<Q::Fetch as Fetch<'w, '_>>::Item, QueryEntityError> {
        let location = world
            .entities
            .get(entity)
            .ok_or(QueryEntityError::NoSuchEntity)?;
        if !self
            .current_query_access_cache()
            .matched_archetypes
            .contains(location.archetype_id.index())
        {
            return Err(QueryEntityError::QueryDoesNotMatch);
        }
        let archetype = &world.archetypes[location.archetype_id];
        let mut fetch = <Q::Fetch as Fetch>::init(
            world,
            &self.fetch_state,
            &self.current_relation_filter.0,
            last_change_tick,
            change_tick,
        );
        let mut filter = <F::Fetch as Fetch>::init(
            world,
            &self.filter_state,
            &self.current_relation_filter.1,
            last_change_tick,
            change_tick,
        );

        fetch.set_archetype(
            &self.fetch_state,
            &self.current_relation_filter.0,
            archetype,
            &world.storages().tables,
        );
        filter.set_archetype(
            &self.filter_state,
            &self.current_relation_filter.1,
            archetype,
            &world.storages().tables,
        );
        if filter.archetype_filter_fetch(location.index) {
            Ok(fetch.archetype_fetch(location.index))
        } else {
            Err(QueryEntityError::QueryDoesNotMatch)
        }
    }

    #[inline]
    pub fn iter<'w, 's>(&'s mut self, world: &'w World) -> QueryIter<'w, 's, Q, F>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFETY: query is read only
        unsafe { self.iter_unchecked(world) }
    }

    #[inline]
    pub fn iter_mut<'w, 's>(&'s mut self, world: &'w mut World) -> QueryIter<'w, 's, Q, F> {
        // SAFETY: query has unique world access
        unsafe { self.iter_unchecked(world) }
    }

    /// # Safety
    ///
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    #[inline]
    pub unsafe fn iter_unchecked<'w, 's>(
        &'s mut self,
        world: &'w World,
    ) -> QueryIter<'w, 's, Q, F> {
        self.validate_world_and_update_archetypes(world);
        self.iter_unchecked_manual(world, world.last_change_tick(), world.read_change_tick())
    }

    /// # Safety
    ///
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    /// This does not validate that `world.id()` matches `self.world_id`. Calling this on a `world`
    /// with a mismatched WorldId is unsound.
    #[inline]
    pub(crate) unsafe fn iter_unchecked_manual<'w, 's>(
        &'s self,
        world: &'w World,
        last_change_tick: u32,
        change_tick: u32,
    ) -> QueryIter<'w, 's, Q, F> {
        QueryIter::new(world, self, last_change_tick, change_tick)
    }

    #[inline]
    pub fn for_each<'s, 'w>(
        &'s mut self,
        world: &'w World,
        func: impl FnMut(<Q::Fetch as Fetch<'w, 's>>::Item),
    ) where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFETY: query is read only
        unsafe {
            self.for_each_unchecked(world, func);
        }
    }

    #[inline]
    pub fn for_each_mut<'w, 's>(
        &'s mut self,
        world: &'w mut World,
        func: impl FnMut(<Q::Fetch as Fetch<'w, 's>>::Item),
    ) {
        // SAFETY: query has unique world access
        unsafe {
            self.for_each_unchecked(world, func);
        }
    }

    /// # Safety
    ///
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    #[inline]
    pub unsafe fn for_each_unchecked<'w, 's>(
        &'s mut self,
        world: &'w World,
        func: impl FnMut(<Q::Fetch as Fetch<'w, 's>>::Item),
    ) {
        self.validate_world_and_update_archetypes(world);
        self.for_each_unchecked_manual(
            world,
            func,
            world.last_change_tick(),
            world.read_change_tick(),
        );
    }

    #[inline]
    pub fn par_for_each<'w, 's>(
        &'s mut self,
        world: &'w World,
        task_pool: &TaskPool,
        batch_size: usize,
        func: impl Fn(<Q::Fetch as Fetch<'w, 's>>::Item) + Send + Sync + Clone,
    ) where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFETY: query is read only
        unsafe {
            self.par_for_each_unchecked(world, task_pool, batch_size, func);
        }
    }

    #[inline]
    pub fn par_for_each_mut<'w, 's>(
        &'s mut self,
        world: &'w mut World,
        task_pool: &TaskPool,
        batch_size: usize,
        func: impl Fn(<Q::Fetch as Fetch<'w, 's>>::Item) + Send + Sync + Clone,
    ) {
        // SAFETY: query has unique world access
        unsafe {
            self.par_for_each_unchecked(world, task_pool, batch_size, func);
        }
    }

    /// # Safety
    ///
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    #[inline]
    pub unsafe fn par_for_each_unchecked<'w, 's>(
        &'s mut self,
        world: &'w World,
        task_pool: &TaskPool,
        batch_size: usize,
        func: impl Fn(<Q::Fetch as Fetch<'w, 's>>::Item) + Send + Sync + Clone,
    ) {
        self.validate_world_and_update_archetypes(world);
        self.par_for_each_unchecked_manual(
            world,
            task_pool,
            batch_size,
            func,
            world.last_change_tick(),
            world.read_change_tick(),
        );
    }

    /// # Safety
    ///
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    /// This does not validate that `world.id()` matches `self.world_id`. Calling this on a `world`
    /// with a mismatched WorldId is unsound.
    pub(crate) unsafe fn for_each_unchecked_manual<'w, 's>(
        &'s self,
        world: &'w World,
        mut func: impl FnMut(<Q::Fetch as Fetch<'w, 's>>::Item),
        last_change_tick: u32,
        change_tick: u32,
    ) {
        let mut fetch = <Q::Fetch as Fetch>::init(
            world,
            &self.fetch_state,
            &self.current_relation_filter.0,
            last_change_tick,
            change_tick,
        );
        let mut filter = <F::Fetch as Fetch>::init(
            world,
            &self.filter_state,
            &self.current_relation_filter.1,
            last_change_tick,
            change_tick,
        );
        if fetch.is_dense() && filter.is_dense() {
            let tables = &world.storages().tables;
            for table_id in self.current_query_access_cache().matched_table_ids.iter() {
                let table = &tables[*table_id];
                fetch.set_table(&self.fetch_state, &self.current_relation_filter.0, table);
                filter.set_table(&self.filter_state, &self.current_relation_filter.1, table);

                for table_index in 0..table.len() {
                    if !filter.table_filter_fetch(table_index) {
                        continue;
                    }
                    let item = fetch.table_fetch(table_index);
                    func(item);
                }
            }
        } else {
            let archetypes = &world.archetypes;
            let tables = &world.storages().tables;
            for archetype_id in self
                .current_query_access_cache()
                .matched_archetype_ids
                .iter()
            {
                let archetype = &archetypes[*archetype_id];
                fetch.set_archetype(
                    &self.fetch_state,
                    &self.current_relation_filter.0,
                    archetype,
                    tables,
                );
                filter.set_archetype(
                    &self.filter_state,
                    &self.current_relation_filter.1,
                    archetype,
                    tables,
                );

                for archetype_index in 0..archetype.len() {
                    if !filter.archetype_filter_fetch(archetype_index) {
                        continue;
                    }
                    func(fetch.archetype_fetch(archetype_index));
                }
            }
        }
    }

    /// # Safety
    ///
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    /// This does not validate that `world.id()` matches `self.world_id`. Calling this on a `world`
    /// with a mismatched WorldId is unsound.
    pub unsafe fn par_for_each_unchecked_manual<'w, 's>(
        &'s self,
        world: &'w World,
        task_pool: &TaskPool,
        batch_size: usize,
        func: impl Fn(<Q::Fetch as Fetch<'w, 's>>::Item) + Send + Sync + Clone,
        last_change_tick: u32,
        change_tick: u32,
    ) {
        task_pool.scope(|scope| {
            let fetch = <Q::Fetch as Fetch>::init(
                world,
                &self.fetch_state,
                &self.current_relation_filter.0,
                last_change_tick,
                change_tick,
            );
            let filter = <F::Fetch as Fetch>::init(
                world,
                &self.filter_state,
                &self.current_relation_filter.1,
                last_change_tick,
                change_tick,
            );

            if fetch.is_dense() && filter.is_dense() {
                let tables = &world.storages().tables;
                for table_id in self.current_query_access_cache().matched_table_ids.iter() {
                    let table = &tables[*table_id];
                    let mut offset = 0;
                    while offset < table.len() {
                        let func = func.clone();
                        scope.spawn(async move {
                            let mut fetch = <Q::Fetch as Fetch>::init(
                                world,
                                &self.fetch_state,
                                &self.current_relation_filter.0,
                                last_change_tick,
                                change_tick,
                            );
                            let mut filter = <F::Fetch as Fetch>::init(
                                world,
                                &self.filter_state,
                                &self.current_relation_filter.1,
                                last_change_tick,
                                change_tick,
                            );
                            let tables = &world.storages().tables;
                            let table = &tables[*table_id];
                            fetch.set_table(
                                &self.fetch_state,
                                &self.current_relation_filter.0,
                                table,
                            );
                            filter.set_table(
                                &self.filter_state,
                                &self.current_relation_filter.1,
                                table,
                            );
                            let len = batch_size.min(table.len() - offset);
                            for table_index in offset..offset + len {
                                if !filter.table_filter_fetch(table_index) {
                                    continue;
                                }
                                let item = fetch.table_fetch(table_index);
                                func(item);
                            }
                        });
                        offset += batch_size;
                    }
                }
            } else {
                let archetypes = &world.archetypes;
                for archetype_id in self
                    .current_query_access_cache()
                    .matched_archetype_ids
                    .iter()
                {
                    let mut offset = 0;
                    let archetype = &archetypes[*archetype_id];
                    while offset < archetype.len() {
                        let func = func.clone();
                        scope.spawn(async move {
                            let mut fetch = <Q::Fetch as Fetch>::init(
                                world,
                                &self.fetch_state,
                                &self.current_relation_filter.0,
                                last_change_tick,
                                change_tick,
                            );
                            let mut filter = <F::Fetch as Fetch>::init(
                                world,
                                &self.filter_state,
                                &self.current_relation_filter.1,
                                last_change_tick,
                                change_tick,
                            );
                            let tables = &world.storages().tables;
                            let archetype = &world.archetypes[*archetype_id];
                            fetch.set_archetype(
                                &self.fetch_state,
                                &self.current_relation_filter.0,
                                archetype,
                                tables,
                            );
                            filter.set_archetype(
                                &self.filter_state,
                                &self.current_relation_filter.1,
                                archetype,
                                tables,
                            );

                            let len = batch_size.min(archetype.len() - offset);
                            for archetype_index in offset..offset + len {
                                if !filter.archetype_filter_fetch(archetype_index) {
                                    continue;
                                }
                                func(fetch.archetype_fetch(archetype_index));
                            }
                        });
                        offset += batch_size;
                    }
                }
            }
        });
    }
}

/// An error that occurs when retrieving a specific [`Entity`]'s query result.
#[derive(Error, Debug)]
pub enum QueryEntityError {
    #[error("The given entity does not have the requested component.")]
    QueryDoesNotMatch,
    #[error("The requested entity does not exist.")]
    NoSuchEntity,
}
