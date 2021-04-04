mod type_info;

pub use type_info::*;

use std::collections::HashMap;

use crate::{prelude::Entity, storage::SparseSetIndex};
use bitflags::bitflags;
use std::{
    alloc::Layout,
    any::{Any, TypeId},
    collections::hash_map::Entry,
};
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
pub struct DataLayout {
    name: String,
    storage_type: StorageType,
    // SAFETY: This must remain private. It must only be set to "true" if this component is actually Send + Sync
    is_send_and_sync: bool,
    type_id: Option<TypeId>,
    layout: Layout,
    drop: unsafe fn(*mut u8),
}

impl DataLayout {
    pub unsafe fn new(
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

    pub fn from_generic<T: Component>(storage_type: StorageType) -> Self {
        Self {
            name: std::any::type_name::<T>().to_string(),
            storage_type,
            is_send_and_sync: true,
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

impl From<TypeInfo> for DataLayout {
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

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct RelationshipId(usize);

impl RelationshipId {
    #[inline]
    pub const fn new(index: usize) -> RelationshipId {
        RelationshipId(index)
    }

    #[inline]
    pub fn index(self) -> usize {
        self.0
    }
}

impl SparseSetIndex for RelationshipId {
    #[inline]
    fn sparse_set_index(&self) -> usize {
        self.index()
    }

    fn get_sparse_set_index(value: usize) -> Self {
        Self(value)
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct Relship {
    kind: RelationshipKindId,
    target: Option<Entity>,
}

impl Relship {
    pub fn new(kind: RelationshipKindId, target: Option<Entity>) -> Self {
        Self { kind, target }
    }
}

#[derive(Debug)]
pub struct RelationshipInfo {
    id: RelationshipId,
    kind: RelationshipKindId,
    target: Option<Entity>,
}

impl RelationshipInfo {
    pub fn id(&self) -> RelationshipId {
        self.id
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct RelationshipKindId(usize);
#[derive(Debug)]
pub struct RelationshipKindInfo {
    data: DataLayout,
    id: RelationshipKindId,
}

impl RelationshipKindInfo {
    pub fn data_layout(&self) -> &DataLayout {
        &self.data
    }

    pub fn id(&self) -> RelationshipKindId {
        self.id
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct DummyInfo {
    rust_type: Option<TypeId>,
    id: DummyId,
}
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct DummyId(usize);

#[derive(Default, Debug)]
pub struct Relationships {
    relationships: Vec<RelationshipInfo>,
    relationship_indices: HashMap<Relship, RelationshipId, fxhash::FxBuildHasher>,

    kinds: Vec<RelationshipKindInfo>,
    // These are only used by bevy. Scripting/dynamic components should
    // use their own hashmap to lookup CustomId -> RelationshipKindId
    component_indices: HashMap<TypeId, RelationshipKindId, fxhash::FxBuildHasher>,
    resource_indices: HashMap<TypeId, RelationshipKindId, fxhash::FxBuildHasher>,
}

#[derive(Debug, Error)]
pub enum RelationshipsError {
    #[error("A relationship of type {0:?} already exists")]
    RelationshipAlreadyExists(Relship),
    #[error("A type id was already registered")]
    TypeIdDummyIdAlreadyExists(TypeId),
}

impl Relationships {
    pub fn relation_kind_of_component(&self, type_id: TypeId) -> Option<RelationshipKindId> {
        self.component_indices.get(&type_id).copied()
    }

    pub fn relation_kind_of_resource(&self, type_id: TypeId) -> Option<RelationshipKindId> {
        self.resource_indices.get(&type_id).copied()
    }

    pub fn new_component_relationship_kind(
        &mut self,
        type_id: TypeId,
        layout: DataLayout,
    ) -> RelationshipKindId {
        let id = RelationshipKindId(self.kinds.len());
        let prev_inserted = self.component_indices.insert(type_id, id);
        assert!(prev_inserted.is_none());
        self.kinds.push(RelationshipKindInfo { data: layout, id });
        id
    }

    pub fn new_resource_relationship_kind(
        &mut self,
        type_id: TypeId,
        layout: DataLayout,
    ) -> RelationshipKindId {
        let id = RelationshipKindId(self.kinds.len());
        let prev_inserted = self.resource_indices.insert(type_id, id);
        assert!(prev_inserted.is_none());
        self.kinds.push(RelationshipKindInfo { data: layout, id });
        id
    }

    pub fn get_component_relationship_kind(&self, type_id: TypeId) -> Option<RelationshipKindId> {
        self.component_indices.get(&type_id).copied()
    }

    pub fn get_resource_relationship_kind(&self, type_id: TypeId) -> Option<RelationshipKindId> {
        self.resource_indices.get(&type_id).copied()
    }

    pub fn get_component_relationship_kind_or_insert(
        &mut self,
        type_id: TypeId,
        layout: DataLayout,
    ) -> RelationshipKindId {
        match self.component_indices.get(&type_id).copied() {
            Some(kind) => kind,
            None => self.new_component_relationship_kind(type_id, layout),
        }
    }

    pub fn get_resource_relationship_kind_or_insert(
        &mut self,
        type_id: TypeId,
        layout: DataLayout,
    ) -> RelationshipKindId {
        match self.component_indices.get(&type_id).copied() {
            Some(kind) => kind,
            None => self.new_resource_relationship_kind(type_id, layout),
        }
    }

    pub(crate) fn register_relationship(
        &mut self,
        relationship: Relship,
    ) -> Result<(&RelationshipKindInfo, &RelationshipInfo), RelationshipsError> {
        let rel_id = RelationshipId(self.relationships.len());

        if let Entry::Occupied(_) = self.relationship_indices.entry(relationship) {
            return Err(RelationshipsError::RelationshipAlreadyExists(relationship));
        }

        self.relationship_indices.insert(relationship, rel_id);
        self.relationships.push(RelationshipInfo {
            id: rel_id,
            kind: relationship.kind,
            target: relationship.target,
        });

        // Safety: Just inserted ^^^
        unsafe { Ok(self.get_relationship_info_unchecked(rel_id)) }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.relationships.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.relationships.len() == 0
    }

    #[inline]
    pub fn get_resource_id(&self, type_id: TypeId) -> Option<RelationshipId> {
        self.get_relationship_id(Relship {
            kind: self.relation_kind_of_resource(type_id)?,
            target: None,
        })
    }
    #[inline]
    pub fn get_resource_info_or_insert<T: Component>(
        &mut self,
    ) -> (&RelationshipKindInfo, &RelationshipInfo) {
        self.get_resource_info_or_insert_with(TypeId::of::<T>(), TypeInfo::of::<T>)
    }
    #[inline]
    pub fn get_non_send_resource_info_or_insert<T: Any>(
        &mut self,
    ) -> (&RelationshipKindInfo, &RelationshipInfo) {
        self.get_resource_info_or_insert_with(
            TypeId::of::<T>(),
            TypeInfo::of_non_send_and_sync::<T>,
        )
    }
    #[inline]
    fn get_resource_info_or_insert_with(
        &mut self,
        type_id: TypeId,
        data_layout: impl FnOnce() -> TypeInfo,
    ) -> (&RelationshipKindInfo, &RelationshipInfo) {
        let kind = match self.relation_kind_of_resource(type_id) {
            Some(id) => id,
            None => self.new_resource_relationship_kind(type_id, data_layout().into()),
        };

        self.get_relationship_info_or_insert_with(Relship { kind, target: None })
    }

    #[inline]
    pub fn get_component_id(&self, type_id: TypeId) -> Option<RelationshipId> {
        self.get_relationship_id(Relship {
            kind: self.relation_kind_of_component(type_id)?,
            target: None,
        })
    }
    #[inline]
    pub fn get_component_info_or_insert<T: Component>(
        &mut self,
    ) -> (&RelationshipKindInfo, &RelationshipInfo) {
        self.get_component_info_or_insert_with(TypeId::of::<T>(), TypeInfo::of::<T>)
    }
    #[inline]
    pub(crate) fn get_component_info_or_insert_with(
        &mut self,
        type_id: TypeId,
        data_layout: impl FnOnce() -> TypeInfo,
    ) -> (&RelationshipKindInfo, &RelationshipInfo) {
        let kind = match self.relation_kind_of_component(type_id) {
            Some(id) => id,
            None => self.new_component_relationship_kind(type_id, data_layout().into()),
        };

        self.get_relationship_info_or_insert_with(Relship { kind, target: None })
    }

    #[inline]
    pub fn get_relationship_id(&self, relationship: Relship) -> Option<RelationshipId> {
        self.relationship_indices.get(&relationship).copied()
    }
    #[inline]
    pub fn get_relationship_info(
        &self,
        id: RelationshipId,
    ) -> Option<(&RelationshipKindInfo, &RelationshipInfo)> {
        let info = self.relationships.get(id.0)?;
        Some((&self.kinds[info.kind.0], info))
    }
    /// # Safety
    /// `id` must be a valid [RelationshipId]
    #[inline]
    pub unsafe fn get_relationship_info_unchecked(
        &self,
        id: RelationshipId,
    ) -> (&RelationshipKindInfo, &RelationshipInfo) {
        debug_assert!(id.index() < self.relationships.len());
        let info = self.relationships.get_unchecked(id.0);
        (&self.kinds[info.kind.0], info)
    }
    #[inline]
    pub fn get_relationship_info_or_insert_with(
        &mut self,
        relationship: Relship,
    ) -> (&RelationshipKindInfo, &RelationshipInfo) {
        let Relationships {
            relationship_indices,
            relationships,
            ..
        } = self;

        let id = *relationship_indices.entry(relationship).or_insert_with(|| {
            let rel_id = RelationshipId(relationships.len());

            relationships.push(RelationshipInfo {
                id: rel_id,
                kind: relationship.kind,
                target: relationship.target,
            });

            rel_id
        });

        // Safety: just inserted
        unsafe { self.get_relationship_info_unchecked(id) }
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
