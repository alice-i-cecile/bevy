mod type_info;

pub use type_info::*;

use std::collections::HashMap;

use crate::storage::SparseSetIndex;
use std::{alloc::Layout, any::TypeId};
use thiserror::Error;

/// A component is data associated with an [`Entity`](crate::entity::Entity). Each entity can have
/// multiple different types of components, but only one of them per type.
///
/// Any type that is `Send + Sync + 'static` automatically implements `Component`.
///
/// Components are added with new entities using [`Commands::spawn`](crate::system::Commands::spawn),
/// or to existing entities with [`EntityCommands::insert`](crate::system::EntityCommands::insert),
/// or their [`World`](crate::world::World) equivalents.
///
/// Components can be accessed in systems by using a [`Query`](crate::system::Query)
/// as one of the arguments.
///
/// Components can be grouped together into a [`Bundle`](crate::bundle::Bundle).
pub trait Component: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> Component for T {}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum StorageType {
    Table,
    SparseSet,
}

impl Default for StorageType {
    fn default() -> Self {
        StorageType::Table
    }
}

#[derive(Debug)]
pub struct ComponentDescriptor {
    name: String,
    storage_type: StorageType,
    // SAFETY: This must remain private. It must only be set to "true" if this component is actually Send + Sync
    is_send_and_sync: bool,
    type_id: Option<TypeId>,
    layout: Layout,
    drop: unsafe fn(*mut u8),
}

impl ComponentDescriptor {
    pub unsafe fn new_dynamic(
        name: Option<String>,
        storage_type: StorageType,
        is_send_and_sync: bool,
        layout: Layout,
        drop: unsafe fn(*mut u8),
    ) -> Self {
        Self {
            name: name.unwrap_or(String::new()),
            storage_type,
            is_send_and_sync,
            type_id: None,
            layout,
            drop,
        }
    }

    pub fn new<T: Component>(storage_type: StorageType) -> Self {
        Self {
            name: std::any::type_name::<T>().to_string(),
            storage_type,
            is_send_and_sync: true,
            type_id: Some(TypeId::of::<T>()),
            layout: Layout::new::<T>(),
            drop: TypeInfo::drop_ptr::<T>,
        }
    }

    pub fn new_non_send_sync<T: 'static>(storage_type: StorageType) -> Self {
        Self {
            name: std::any::type_name::<T>().to_string(),
            storage_type,
            is_send_and_sync: false,
            type_id: Some(TypeId::of::<T>()),
            layout: Layout::new::<T>(),
            drop: TypeInfo::drop_ptr::<T>,
        }
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn type_id(&self) -> Option<TypeId> {
        self.type_id
    }

    #[inline]
    pub fn layout(&self) -> Layout {
        self.layout
    }

    #[inline]
    pub fn drop(&self) -> unsafe fn(*mut u8) {
        self.drop
    }

    #[inline]
    pub fn storage_type(&self) -> StorageType {
        self.storage_type
    }

    #[inline]
    pub fn is_send_and_sync(&self) -> bool {
        self.is_send_and_sync
    }
}

impl From<TypeInfo> for ComponentDescriptor {
    fn from(type_info: TypeInfo) -> Self {
        Self {
            name: type_info.type_name().to_string(),
            storage_type: StorageType::default(),
            is_send_and_sync: type_info.is_send_and_sync(),
            type_id: Some(type_info.type_id()),
            drop: type_info.drop(),
            layout: type_info.layout(),
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, PartialOrd, Ord)]
pub struct RelationKindId(usize);

impl SparseSetIndex for RelationKindId {
    #[inline]
    fn sparse_set_index(&self) -> usize {
        self.0
    }

    fn get_sparse_set_index(value: usize) -> Self {
        Self(value)
    }
}

#[derive(Debug)]
pub struct RelationKindInfo {
    data: ComponentDescriptor,
    id: RelationKindId,
}

impl RelationKindInfo {
    pub fn data_layout(&self) -> &ComponentDescriptor {
        &self.data
    }

    pub fn id(&self) -> RelationKindId {
        self.id
    }
}

#[derive(Debug, Default)]
pub struct Components {
    kinds: Vec<RelationKindInfo>,
    // These are only used by bevy. Scripting/dynamic components should
    // use their own hashmap to lookup CustomId -> RelationKindId
    component_indices: HashMap<TypeId, RelationKindId, fxhash::FxBuildHasher>,
    resource_indices: HashMap<TypeId, RelationKindId, fxhash::FxBuildHasher>,
}

// FIXME(Relationships) actually return this from functions instead of panic'ing
#[derive(Debug, Error)]
pub enum RelationsError {
    #[error("A component of type {name:?} ({type_id:?}) already exists")]
    ComponentAlreadyExists { type_id: TypeId, name: String },
    #[error("A resource of type {name:?} ({type_id:?}) already exists")]
    ResourceAlreadyExists { type_id: TypeId, name: String },
}

impl Components {
    pub fn new_relation_kind(&mut self, layout: ComponentDescriptor) -> &RelationKindInfo {
        let id = RelationKindId(self.kinds.len());
        self.kinds.push(RelationKindInfo { data: layout, id });
        self.kinds.last().unwrap()
    }

    pub fn new_component_kind(&mut self, layout: ComponentDescriptor) -> &RelationKindInfo {
        let id = RelationKindId(self.kinds.len());
        let prev_inserted = self.component_indices.insert(layout.type_id().unwrap(), id);
        assert!(prev_inserted.is_none());
        self.kinds.push(RelationKindInfo { data: layout, id });
        self.kinds.last().unwrap()
    }

    pub fn new_resource_kind(&mut self, layout: ComponentDescriptor) -> &RelationKindInfo {
        let id = RelationKindId(self.kinds.len());
        let prev_inserted = self.resource_indices.insert(layout.type_id().unwrap(), id);
        assert!(prev_inserted.is_none());
        self.kinds.push(RelationKindInfo { data: layout, id });
        self.kinds.last().unwrap()
    }

    pub fn get_relation_kind(&self, id: RelationKindId) -> &RelationKindInfo {
        self.kinds.get(id.0).unwrap()
    }

    pub fn get_component_kind(&self, type_id: TypeId) -> Option<&RelationKindInfo> {
        let id = self.component_indices.get(&type_id).copied()?;
        Some(&self.kinds[id.0])
    }

    pub fn get_resource_kind(&self, type_id: TypeId) -> Option<&RelationKindInfo> {
        let id = self.resource_indices.get(&type_id).copied()?;
        Some(&self.kinds[id.0])
    }

    pub fn get_component_kind_or_insert(
        &mut self,
        layout: ComponentDescriptor,
    ) -> &RelationKindInfo {
        match self
            .component_indices
            .get(&layout.type_id().unwrap())
            .copied()
        {
            Some(kind) => &self.kinds[kind.0],
            None => self.new_component_kind(layout),
        }
    }

    pub fn get_resource_kind_or_insert(
        &mut self,
        layout: ComponentDescriptor,
    ) -> &RelationKindInfo {
        match self
            .resource_indices
            .get(&layout.type_id().unwrap())
            .copied()
        {
            Some(kind) => &self.kinds[kind.0],
            None => self.new_resource_kind(layout),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ComponentTicks {
    pub(crate) added: u32,
    pub(crate) changed: u32,
}

impl ComponentTicks {
    #[inline]
    pub fn is_added(&self, last_change_tick: u32, change_tick: u32) -> bool {
        // The comparison is relative to `change_tick` so that we can detect changes over the whole
        // `u32` range. Comparing directly the ticks would limit to half that due to overflow
        // handling.
        let component_delta = change_tick.wrapping_sub(self.added);
        let system_delta = change_tick.wrapping_sub(last_change_tick);

        component_delta < system_delta
    }

    #[inline]
    pub fn is_changed(&self, last_change_tick: u32, change_tick: u32) -> bool {
        let component_delta = change_tick.wrapping_sub(self.changed);
        let system_delta = change_tick.wrapping_sub(last_change_tick);

        component_delta < system_delta
    }

    pub(crate) fn new(change_tick: u32) -> Self {
        Self {
            added: change_tick,
            changed: change_tick,
        }
    }

    pub(crate) fn check_ticks(&mut self, change_tick: u32) {
        check_tick(&mut self.added, change_tick);
        check_tick(&mut self.changed, change_tick);
    }

    /// Manually sets the change tick.
    /// Usually, this is done automatically via the [`DerefMut`](std::ops::DerefMut) implementation
    /// on [`Mut`](crate::world::Mut) or [`ResMut`](crate::system::ResMut) etc.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use bevy_ecs::{world::World, component::ComponentTicks};
    /// let world: World = unimplemented!();
    /// let component_ticks: ComponentTicks = unimplemented!();
    ///
    /// component_ticks.set_changed(world.read_change_tick());
    /// ```
    #[inline]
    pub fn set_changed(&mut self, change_tick: u32) {
        self.changed = change_tick;
    }
}

fn check_tick(last_change_tick: &mut u32, change_tick: u32) {
    let tick_delta = change_tick.wrapping_sub(*last_change_tick);
    const MAX_DELTA: u32 = (u32::MAX / 4) * 3;
    // Clamp to max delta
    if tick_delta > MAX_DELTA {
        *last_change_tick = change_tick.wrapping_sub(MAX_DELTA);
    }
}
