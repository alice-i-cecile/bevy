use bevy_app::Plugin;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::{
    component::Component,
    entity::Entity,
    observer::Trigger,
    query::With,
    reflect::ReflectComponent,
    system::{Local, Query, ResMut, Resource, SystemState},
    world::{Mut, OnAdd, OnRemove, World},
};
use bevy_reflect::Reflect;

/// A plugin that synchronizes entities with [`SyncToRenderWorld`] between the main world and the render world.
///
/// Bevy's renderer is architected independently from the main app.
/// It operates in its own separate ECS [`World`], so the renderer logic can run in parallel with the main world logic.
/// This is called "Pipelined Rendering", see [`PipelinedRenderingPlugin`] for more information.
///
/// [`WorldSyncPlugin`] is the first thing that runs every frame and it maintains an entity-to-entity mapping
/// between the main world and the render world.
/// It does so by spawning and despawning entities in the render world, to match spawned and despawned entities in the main world.
/// The link between synced entities is maintained by the [`RenderEntity`] and [`MainEntity`] components.
/// The [`RenderEntity`] contains the corresponding render world entity of a main world entity, while [`MainEntity`] contains
/// the corresponding main world entity of a render world entity.
/// The entities can be accessed by calling `.id()` on either component.
///
/// Synchronization is necessary preparation for extraction ([`ExtractSchedule`](crate::ExtractSchedule)), which copies over component data from the main
/// to the render world for these entities.
///
/// ```text
/// |--------------------------------------------------------------------|
/// |      |         |          Main world update                        |
/// | sync | extract |---------------------------------------------------|
/// |      |         |         Render world update                       |
/// |--------------------------------------------------------------------|
/// ```
///
/// An example for synchronized main entities 1v1 and 18v1
///
/// ```text
/// |---------------------------Main World------------------------------|
/// |  Entity  |                    Component                           |
/// |-------------------------------------------------------------------|
/// | ID: 1v1  | PointLight | RenderEntity(ID: 3V1) | SyncToRenderWorld |
/// | ID: 18v1 | PointLight | RenderEntity(ID: 5V1) | SyncToRenderWorld |
/// |-------------------------------------------------------------------|
///
/// |----------Render World-----------|
/// |  Entity  |       Component      |
/// |---------------------------------|
/// | ID: 3v1  | MainEntity(ID: 1V1)  |
/// | ID: 5v1  | MainEntity(ID: 18V1) |
/// |---------------------------------|
///
/// ```
///
/// Note that this effectively establishes a link between the main world entity and the render world entity.
/// Not every entity needs to be synchronized, however; only entities with the [`SyncToRenderWorld`] component are synced.
/// Adding [`SyncToRenderWorld`] to a main world component will establish such a link.
/// Once a synchronized main entity is despawned, its corresponding render entity will be automatically
/// despawned in the next `sync`.
///
/// The sync step does not copy any of component data between worlds, since its often not necessary to transfer over all
/// the components of a main world entity.
/// The render world probably cares about a `Position` component, but not a `Velocity` component.
/// The extraction happens in its own step, independently from, and after synchronization.
///
/// Moreover, [`WorldSyncPlugin`] only synchronizes *entities*. [`RenderAsset`](crate::render_asset::RenderAsset)s like meshes and textures are handled
/// differently.
///
/// [`PipelinedRenderingPlugin`]: crate::pipelined_rendering::PipelinedRenderingPlugin
#[derive(Default)]
pub struct WorldSyncPlugin;

impl Plugin for WorldSyncPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        app.init_resource::<PendingSyncEntity>();
        app.observe(
            |trigger: Trigger<OnAdd, SyncToRenderWorld>, mut pending: ResMut<PendingSyncEntity>| {
                pending.push(EntityRecord::Added(trigger.entity()));
            },
        );
        app.observe(
            |trigger: Trigger<OnRemove, SyncToRenderWorld>,
             mut pending: ResMut<PendingSyncEntity>,
             query: Query<&RenderEntity>| {
                if let Ok(e) = query.get(trigger.entity()) {
                    pending.push(EntityRecord::Removed(e.id()));
                };
            },
        );
    }
}
/// Marker component that indicates that its entity needs to be synchronized to the render world
///
/// NOTE: This component should persist throughout the entity's entire lifecycle.
/// If this component is removed from its entity, the entity will be despawned.
#[derive(Component, Clone, Debug, Default, Reflect)]
#[reflect[Component]]
#[component(storage = "SparseSet")]
pub struct SyncToRenderWorld;

/// Component added on the main world entities that are synced to the Render World in order to keep track of the corresponding render world entity
#[derive(Component, Deref, Clone, Debug, Copy)]
pub struct RenderEntity(Entity);
impl RenderEntity {
    #[inline]
    pub fn id(&self) -> Entity {
        self.0
    }
}

/// Component added on the render world entities to keep track of the corresponding main world entity
#[derive(Component, Deref, Clone, Debug)]
pub struct MainEntity(Entity);
impl MainEntity {
    #[inline]
    pub fn id(&self) -> Entity {
        self.0
    }
}

/// Marker component that indicates that its entity needs to be despawned at the end of the frame.
#[derive(Component, Clone, Debug, Default, Reflect)]
#[component(storage = "SparseSet")]
pub struct TemporaryRenderEntity;

/// A record enum to what entities with [`SyncToRenderWorld`] have been added or removed.
pub(crate) enum EntityRecord {
    /// When an entity is spawned on the main world, notify the render world so that it can spawn a corresponding
    /// entity. This contains the main world entity.
    Added(Entity),
    /// When an entity is despawned on the main world, notify the render world so that the corresponding entity can be
    /// despawned. This contains the render world entity.
    Removed(Entity),
}

// Entity Record in MainWorld pending to Sync
#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct PendingSyncEntity {
    records: Vec<EntityRecord>,
}

pub(crate) fn entity_sync_system(main_world: &mut World, render_world: &mut World) {
    main_world.resource_scope(|world, mut pending: Mut<PendingSyncEntity>| {
        // TODO : batching record
        for record in pending.drain(..) {
            match record {
                EntityRecord::Added(e) => {
                    if let Ok(mut entity) = world.get_entity_mut(e) {
                        match entity.entry::<RenderEntity>() {
                            bevy_ecs::world::Entry::Occupied(_) => {
                                panic!("Attempting to synchronize an entity that has already been synchronized!");
                            }
                            bevy_ecs::world::Entry::Vacant(entry) => {
                                let id = render_world.spawn(MainEntity(e)).id();

                                entry.insert(RenderEntity(id));
                            }
                        };
                    }
                }
                EntityRecord::Removed(e) => {
                    if let Ok(ec) = render_world.get_entity_mut(e) {
                        ec.despawn();
                    };
                }
            }
        }
    });
}

pub(crate) fn despawn_temporary_render_entities(
    world: &mut World,
    state: &mut SystemState<Query<Entity, With<TemporaryRenderEntity>>>,
    mut local: Local<Vec<Entity>>,
) {
    let query = state.get(world);

    local.extend(query.iter());

    // Ensure next frame allocation keeps order
    local.sort_unstable_by_key(|e| e.index());
    for e in local.drain(..).rev() {
        world.despawn(e);
    }
}

/// This module exists to keep the complex unsafe code out of the main module.
///
/// The implementations for both [`MainEntity`] and [`RenderEntity`] should stay in sync,
/// and are based off of the `&T` implementation in `bevy_ecs`.
mod render_entities_world_query_impls {
    use super::RenderEntity;

    use bevy_ecs::{
        archetype::Archetype,
        component::{ComponentId, Components, Tick},
        entity::Entity,
        ptr::ThinSlicePtr,
        query::{FilteredAccess, QueryData, ReadOnlyQueryData, WorldQuery},
        storage::{Table, TableRow},
        world::{unsafe_world_cell::UnsafeWorldCell, World},
    };

    use bevy_utils::syncunsafecell::UnsafeCell;

    /// A [`WorldQuery::Fetch`] type that stores the state needed to read a component from a [`Table`].
    ///
    /// This type is a table-specialized variant of the private `ReadFetch` from [`bevy_ecs`].
    ///
    /// # Warning
    ///
    /// It is only sound to use this type as a `Fetch` if the component uses [`StorageType::Table`](bevy_ecs::component::StorageType::Table).
    #[derive(Clone)]
    struct TableFetch<'w, T> {
        table_components: Option<ThinSlicePtr<'w, UnsafeCell<T>>>,
    }

    impl<'w, T> TableFetch<'w, T> {
        fn extract(&mut self) {
            todo!()
        }
    }

    unsafe impl WorldQuery for RenderEntity {
        type Item<'w> = Entity;
        type Fetch<'w> = TableFetch<'w, RenderEntity>;
        type State = ComponentId;

        fn shrink<'wlong: 'wshort, 'wshort>(item: Entity) -> Entity {
            item
        }

        fn shrink_fetch<'wlong: 'wshort, 'wshort>(
            fetch: Self::Fetch<'wlong>,
        ) -> Self::Fetch<'wshort> {
            fetch
        }

        #[inline]
        unsafe fn init_fetch<'w>(
            _world: UnsafeWorldCell<'w>,
            _component_id: &ComponentId,
            _last_run: Tick,
            _this_run: Tick,
        ) -> TableFetch<'w, RenderEntity> {
            TableFetch {
                table_components: None,
            }
        }

        const IS_DENSE: bool = true;

        #[inline]
        unsafe fn set_archetype<'w>(
            fetch: &mut TableFetch<'w, RenderEntity>,
            component_id: &ComponentId,
            _archetype: &'w Archetype,
            table: &'w Table,
        ) {
            if Self::IS_DENSE {
                // SAFETY: `set_archetype`'s safety rules are a super set of the `set_table`'s ones.
                unsafe {
                    Self::set_table(fetch, component_id, table);
                }
            }
        }

        #[inline]
        unsafe fn set_table<'w>(
            fetch: &mut TableFetch<'w, RenderEntity>,
            &component_id: &ComponentId,
            table: &'w Table,
        ) {
            // SAFETY: T::STORAGE_TYPE is always StorageType::Table for `RenderEntity`.
            unsafe {
                fetch.table_components =
                    Some(table.get_data_slice_for(component_id).unwrap().into());
            }
        }

        #[inline(always)]
        unsafe fn fetch<'w>(
            fetch: &mut Self::Fetch<'w>,
            _entity: Entity,
            table_row: TableRow,
        ) -> Entity {
            let table = fetch.table_components.unwrap();
            // SAFETY: Caller ensures `table_row` is in range.
            let unsafe_cell = unsafe { table.get(table_row.as_usize()) };
            let pointer = unsafe_cell.get();
            // SAFETY: TODO, please help me figure out if this is sound
            let component = unsafe { &*pointer };
            component.id()
        }

        fn update_component_access(
            &component_id: &ComponentId,
            access: &mut FilteredAccess<ComponentId>,
        ) {
            assert!(
            !access.access().has_component_write(component_id),
            "&{} conflicts with a previous access in this query. Shared access cannot coincide with exclusive access.",
            core::any::type_name::<RenderEntity>(),
        );
            access.add_component_read(component_id);
        }

        fn init_state(world: &mut World) -> ComponentId {
            world.register_component::<RenderEntity>()
        }

        fn get_state(components: &Components) -> Option<Self::State> {
            components.component_id::<RenderEntity>()
        }

        fn matches_component_set(
            &state: &ComponentId,
            set_contains_id: &impl Fn(ComponentId) -> bool,
        ) -> bool {
            set_contains_id(state)
        }
    }

    // SAFETY: Component access of Self::ReadOnly is a subset of Self.
    // Self::ReadOnly matches exactly the same archetypes/tables as Self.
    unsafe impl QueryData for RenderEntity {
        type ReadOnly = RenderEntity;
    }

    // SAFETY: the underlying `Entity` is copied, and no mutable access is provided.
    unsafe impl ReadOnlyQueryData for RenderEntity {}
}

#[cfg(test)]
mod tests {
    use bevy_ecs::{
        component::Component,
        entity::Entity,
        observer::Trigger,
        query::With,
        system::{Query, ResMut},
        world::{OnAdd, OnRemove, World},
    };

    use super::{
        entity_sync_system, EntityRecord, MainEntity, PendingSyncEntity, RenderEntity,
        SyncToRenderWorld,
    };

    #[derive(Component)]
    struct RenderDataComponent;

    #[test]
    fn world_sync() {
        let mut main_world = World::new();
        let mut render_world = World::new();
        main_world.init_resource::<PendingSyncEntity>();

        main_world.observe(
            |trigger: Trigger<OnAdd, SyncToRenderWorld>, mut pending: ResMut<PendingSyncEntity>| {
                pending.push(EntityRecord::Added(trigger.entity()));
            },
        );
        main_world.observe(
            |trigger: Trigger<OnRemove, SyncToRenderWorld>,
             mut pending: ResMut<PendingSyncEntity>,
             query: Query<&RenderEntity>| {
                if let Ok(e) = query.get(trigger.entity()) {
                    pending.push(EntityRecord::Removed(e.id()));
                };
            },
        );

        // spawn some empty entities for test
        for _ in 0..99 {
            main_world.spawn_empty();
        }

        // spawn
        let main_entity = main_world
            .spawn(RenderDataComponent)
            // indicates that its entity needs to be synchronized to the render world
            .insert(SyncToRenderWorld)
            .id();

        entity_sync_system(&mut main_world, &mut render_world);

        let mut q = render_world.query_filtered::<Entity, With<MainEntity>>();

        // Only one synchronized entity
        assert!(q.iter(&render_world).count() == 1);

        let render_entity = q.get_single(&render_world).unwrap();
        let render_entity_component = main_world.get::<RenderEntity>(main_entity).unwrap();

        assert!(render_entity_component.id() == render_entity);

        let main_entity_component = render_world
            .get::<MainEntity>(render_entity_component.id())
            .unwrap();

        assert!(main_entity_component.id() == main_entity);

        // despawn
        main_world.despawn(main_entity);

        entity_sync_system(&mut main_world, &mut render_world);

        // Only one synchronized entity
        assert!(q.iter(&render_world).count() == 0);
    }
}
