pub use crate::change_detection::{NonSendMut, ResMut};
use crate::{
    self as bevy_ecs,
    archetype::{Archetype, ArchetypeComponentId, Archetypes},
    bundle::Bundles,
    cell::SemiSafeCell,
    change_detection::Ticks,
    component::{Component, ComponentId, ComponentTicks, Components},
    entity::{Entities, Entity},
    query::{
        FilterFetch, FilteredAccess, FilteredAccessSet, QueryState, ReadOnlyFetch, WorldQuery,
    },
    system::{CommandQueue, Commands, Query, SystemMeta},
    world::{FromWorld, World},
};
pub use bevy_ecs_macros::SystemParam;
use bevy_ecs_macros::{all_tuples, impl_param_set};
use std::{
    fmt::Debug,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

/// A function parameter that can be used in a valid [`System`](super::System).
///
/// # Derive
///
/// This trait can be derived with [`#[derive(SystemParam)]`](`derive@super::SystemParam`), but note
/// that every struct field must also implement `SystemParam`.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// use std::marker::PhantomData;
/// use bevy_ecs::system::SystemParam;
///
/// #[derive(SystemParam)]
/// struct MyParam<'w, 's> {
///     foo: Res<'w, usize>,
///     #[system_param(ignore)]
///     marker: PhantomData<&'s usize>,
/// }
///
/// fn my_system(param: MyParam) {
///     // Access the resource through `param.foo`
/// }
///
/// # bevy_ecs::system::assert_is_system(my_system);
/// ```
pub trait SystemParam: Sized {
    type Fetch: for<'w, 's> SystemParamFetch<'w, 's>;
}

/// A convenient type alias for the [`Item`](SystemParamFetch::Item) retrieved by a [`SystemParam`].
pub type SystemParamItem<'w, 's, P> = <<P as SystemParam>::Fetch as SystemParamFetch<'w, 's>>::Item;

/// Types that can represent the internal state of a [`SystemParam`].
///
/// # Safety
///
/// The implementor must ensure:
/// - Initialization specifies the param's *exact* [`World`] access.
/// - Initialization fails if the param's access conflicts with another, previously initialized param.
pub unsafe trait SystemParamState: Send + Sync + 'static {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self;
    #[inline]
    fn new_archetype(&mut self, _archetype: &Archetype, _system_meta: &mut SystemMeta) {}
    #[inline]
    fn apply(&mut self, _world: &mut World) {}
}

/// Types that can retrieve the data represented by the [`SystemParam`] from a [`World`].
pub trait SystemParamFetch<'world, 'state>: SystemParamState {
    /// The type retrieved by the param.
    type Item: SystemParam<Fetch = Self>;

    /// # Safety
    ///
    /// The caller must ensure:
    /// - The given world is the same world used to initialize the param.
    /// - There are no active references that conflict with the param's access. Mutable access must be unique.
    unsafe fn get_param(
        state: &'state mut Self,
        system_meta: &SystemMeta,
        world: SemiSafeCell<'world, World>,
        change_tick: u32,
    ) -> Self::Item;
}

/// [`Fetch`](SystemParam::Fetch) types that access [`World`] data immutably (or not at all).
///
/// # Safety
///
/// The implementor must ensure that this is only implemented for types that fit the criteria.
pub unsafe trait ReadOnlySystemParamFetch {}

/// A non-existent component accessed by systems with params that hold a
/// [`World`](crate::world::World) reference.
///
/// Current list:
/// - [`Query`](crate::system::Query)
/// - [`Commands`](crate::system::Commands)
/// - [`&World`](crate::world::World)
/// - [`&mut World`](crate::world::World)
/// - [`&Archetypes`](crate::archetype::Archetypes)
/// - [`&Bundles`](crate::bundle::Bundles)
/// - [`&Components`](crate::component::Components)
/// - [`&Entities`](crate::entity::Entities)
/// - [`ParamSet`] (if includes any of above)
#[derive(Component)]
pub(crate) struct WorldAccess;

/// Marks system as having a param that holds a `&World` reference.
fn add_shared_world_access(
    world: &mut World,
    system_meta: &mut SystemMeta,
    read_all: bool,
    param_name: &str,
) {
    let id = world.init_component::<WorldAccess>();
    let mut world_access = FilteredAccess::default();
    world_access.add_read(id);

    if read_all {
        world_access.read_all();
    }

    // conflict with &mut World (if it appears first)
    if !system_meta
        .component_access_set
        .get_conflicts(&world_access)
        .is_empty()
    {
        panic!(
            "{} conflicts with another system param in {}. \
            Mutable access must be unique.",
            param_name, system_meta.name,
        );
    }

    // conflict with any param that holds &mut World (if this param appears first)
    system_meta.component_access_set.add(world_access);

    // ensures executor sees conflict with another system having &mut World
    system_meta
        .archetype_component_access
        .add_read(ArchetypeComponentId::WORLD_ACCESS);

    if read_all {
        // prevent executor from running in parallel with systems that mutably borrow data
        system_meta.archetype_component_access.read_all();
    }
}

/// Marks system as having a param that holds a `&mut World` reference.
fn add_exclusive_world_access(world: &mut World, system_meta: &mut SystemMeta, param_name: &str) {
    let id = world.init_component::<WorldAccess>();
    let mut world_access = FilteredAccess::default();
    world_access.add_write(id);
    world_access.write_all();

    // conflict with &World (if it appears first)
    if !system_meta
        .component_access_set
        .get_conflicts(&world_access)
        .is_empty()
    {
        panic!(
            "{} conflicts with another system param in {}. \
            Mutable access must be unique.",
            param_name, system_meta.name,
        );
    }

    // conflict with any param that holds &World (if this param appears first)
    system_meta.component_access_set.add(world_access);

    // ensures executor sees conflict with another system holding &World
    system_meta
        .archetype_component_access
        .add_write(ArchetypeComponentId::WORLD_ACCESS);

    // prevent executor from running in parallel with systems that borrow data
    system_meta.archetype_component_access.write_all();
}

impl<'w, 's, Q: WorldQuery + 'static, F: WorldQuery + 'static> SystemParam for Query<'w, 's, Q, F>
where
    F::Fetch: FilterFetch,
{
    type Fetch = QueryState<Q, F>;
}

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl<Q: WorldQuery + 'static, F: WorldQuery + 'static> SystemParamState for QueryState<Q, F>
where
    F::Fetch: FilterFetch,
{
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        let state = QueryState::new(world);
        assert_component_access_compatibility(
            &system_meta.name,
            std::any::type_name::<Q>(),
            std::any::type_name::<F>(),
            &system_meta.component_access_set,
            &state.component_access,
            world,
        );

        system_meta
            .component_access_set
            .add(state.component_access.clone());
        system_meta
            .archetype_component_access
            .extend(&state.archetype_component_access);

        let param_name = format!(
            "Query<{}, {}>",
            std::any::type_name::<Q>(),
            std::any::type_name::<F>()
        );
        add_shared_world_access(world, system_meta, false, param_name.as_ref());

        state
    }

    fn new_archetype(&mut self, archetype: &Archetype, system_meta: &mut SystemMeta) {
        self.new_archetype(archetype);
        system_meta
            .archetype_component_access
            .extend(&self.archetype_component_access);
    }
}

impl<'w, 's, Q: WorldQuery + 'static, F: WorldQuery + 'static> SystemParamFetch<'w, 's>
    for QueryState<Q, F>
where
    F::Fetch: FilterFetch,
{
    type Item = Query<'w, 's, Q, F>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        change_tick: u32,
    ) -> Self::Item {
        Query::new(
            world.as_ref(),
            state,
            system_meta.last_change_tick,
            change_tick,
        )
    }
}

// SAFETY: QueryState is limited to read-only fetches.
unsafe impl<Q: WorldQuery, F: WorldQuery> ReadOnlySystemParamFetch for QueryState<Q, F>
where
    Q::Fetch: ReadOnlyFetch,
    F::Fetch: FilterFetch,
{
}

fn assert_component_access_compatibility(
    system_name: &str,
    query_type: &'static str,
    filter_type: &'static str,
    system_access: &FilteredAccessSet<ComponentId>,
    current: &FilteredAccess<ComponentId>,
    world: &World,
) {
    let mut conflicts = system_access.get_conflicts(current);
    if conflicts.is_empty() {
        return;
    }
    let conflicting_components = conflicts
        .drain(..)
        .map(|component_id| world.components.get_info(component_id).unwrap().name())
        .collect::<Vec<&str>>();
    let accesses = conflicting_components.join(", ");
    panic!("error[B0001]: Query<{}, {}> in system {} accesses component(s) {} in a way that conflicts with a previous system parameter. Consider using `Without<T>` to create disjoint Queries or merging conflicting Queries into a `ParamSet`.",
           query_type, filter_type, system_name, accesses);
}

/// [`SystemParam`] set that can contain params with conflicting access, with the caveat that only
/// one param is accessible at a time.
pub struct ParamSet<'w, 's, T: SystemParam> {
    param_states: &'s mut T::Fetch,
    world: SemiSafeCell<'w, World>,
    system_meta: SystemMeta,
    change_tick: u32,
}

/// The [`SystemParamState`] of [`ParamSet<(...)>`](ParamSet).
pub struct ParamSetState<T: for<'w, 's> SystemParamFetch<'w, 's>>(T);

impl_param_set!();

/// Types that are singletons. A [`World`] can have most one instance of these types.
pub trait Resource: Send + Sync + 'static {}

impl<T> Resource for T where T: Send + Sync + 'static {}

/// Shared borrow of resource.
///
/// See the [`World`](crate::world::World) documentation to see the usage of a resource.
///
/// For a unique, mutable borrow, see [`ResMut<T>`].
///
/// # Panics
///
/// Panics when used as a `SystemParam` if `T` has not be inserted as a resource.
///
/// Use `Option<Res<T>>` instead if the resource might not always exist.
pub struct Res<'w, T: Resource> {
    value: &'w T,
    ticks: &'w ComponentTicks,
    last_change_tick: u32,
    change_tick: u32,
}

impl<'w, T: Resource> Debug for Res<'w, T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Res").field(&self.value).finish()
    }
}

impl<'w, T: Resource> Res<'w, T> {
    /// Returns `true` if the resource was added after the system last ran.
    pub fn is_added(&self) -> bool {
        self.ticks.is_added(self.last_change_tick, self.change_tick)
    }

    /// Returns `true` if the resource was added or mutably-dereferenced after the system last ran.
    pub fn is_changed(&self) -> bool {
        self.ticks
            .is_changed(self.last_change_tick, self.change_tick)
    }

    pub fn into_inner(self) -> &'w T {
        self.value
    }
}

impl<'w, T: Resource> Deref for Res<'w, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<'w, T: Resource> AsRef<T> for Res<'w, T> {
    #[inline]
    fn as_ref(&self) -> &T {
        self.deref()
    }
}

impl<'w, T: Resource> From<ResMut<'w, T>> for Res<'w, T> {
    fn from(res: ResMut<'w, T>) -> Self {
        Self {
            value: res.value,
            ticks: res.ticks.component_ticks,
            change_tick: res.ticks.change_tick,
            last_change_tick: res.ticks.last_change_tick,
        }
    }
}

// SAFETY: non-mutable borrow
unsafe impl<T: Resource> ReadOnlySystemParamFetch for ResState<T> {}

/// The [`SystemParamState`] of [`Res<T>`].
#[doc(hidden)]
pub struct ResState<T> {
    component_id: ComponentId,
    marker: PhantomData<T>,
}

impl<'a, T: Resource> SystemParam for Res<'a, T> {
    type Fetch = ResState<T>;
}

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl<T: Resource> SystemParamState for ResState<T> {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        let component_id = world.initialize_resource::<T>();
        let combined_access = system_meta.component_access_set.combined_access_mut();
        assert!(
            !combined_access.has_write(component_id),
            "error[B0002]: Res<{}> in system {} conflicts with a previous ResMut<{0}> access. Consider removing the duplicate access.",
            std::any::type_name::<T>(),
            system_meta.name,
        );
        combined_access.add_read(component_id);

        let resource_archetype = world.archetypes.resource();
        let archetype_component_id = resource_archetype
            .get_archetype_component_id(component_id)
            .unwrap();
        system_meta
            .archetype_component_access
            .add_read(archetype_component_id);
        Self {
            component_id,
            marker: PhantomData,
        }
    }
}

impl<'w, 's, T: Resource> SystemParamFetch<'w, 's> for ResState<T> {
    type Item = Res<'w, T>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        change_tick: u32,
    ) -> Self::Item {
        let column = world
            .as_ref()
            .get_populated_resource_column(state.component_id)
            .unwrap_or_else(|| {
                panic!(
                    "Resource requested by {} does not exist: {}",
                    system_meta.name,
                    std::any::type_name::<T>()
                )
            });
        Res {
            value: &*column.get_data_ptr().cast::<T>().as_ptr(),
            ticks: column.get_ticks_unchecked(0),
            last_change_tick: system_meta.last_change_tick,
            change_tick,
        }
    }
}

/// The [`SystemParamState`] of [`Option<Res<T>>`](`Res`).
#[doc(hidden)]
pub struct OptionResState<T>(ResState<T>);

impl<'a, T: Resource> SystemParam for Option<Res<'a, T>> {
    type Fetch = OptionResState<T>;
}

// SAFETY: See ResState<T>
unsafe impl<T: Resource> SystemParamState for OptionResState<T> {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        Self(ResState::init(world, system_meta))
    }
}

impl<'w, 's, T: Resource> SystemParamFetch<'w, 's> for OptionResState<T> {
    type Item = Option<Res<'w, T>>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        change_tick: u32,
    ) -> Self::Item {
        world
            .as_ref()
            .get_populated_resource_column(state.0.component_id)
            .map(|column| Res {
                value: &*column.get_data_ptr().cast::<T>().as_ptr(),
                ticks: column.get_ticks_unchecked(0),
                last_change_tick: system_meta.last_change_tick,
                change_tick,
            })
    }
}

// SAFETY: non-mutable borrow
unsafe impl<T: Resource> ReadOnlySystemParamFetch for OptionResState<T> {}

/// The [`SystemParamState`] of [`ResMut<T>`].
#[doc(hidden)]
pub struct ResMutState<T> {
    component_id: ComponentId,
    marker: PhantomData<T>,
}

impl<'a, T: Resource> SystemParam for ResMut<'a, T> {
    type Fetch = ResMutState<T>;
}

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl<T: Resource> SystemParamState for ResMutState<T> {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        let component_id = world.initialize_resource::<T>();
        let combined_access = system_meta.component_access_set.combined_access_mut();
        if combined_access.has_write(component_id) {
            panic!(
                "error[B0002]: ResMut<{}> in system {} conflicts with a previous ResMut<{0}> access. Consider removing the duplicate access.",
                std::any::type_name::<T>(), system_meta.name);
        } else if combined_access.has_read(component_id) {
            panic!(
                "error[B0002]: ResMut<{}> in system {} conflicts with a previous Res<{0}> access. Consider removing the duplicate access.",
                std::any::type_name::<T>(), system_meta.name);
        }
        combined_access.add_write(component_id);

        let resource_archetype = world.archetypes.resource();
        let archetype_component_id = resource_archetype
            .get_archetype_component_id(component_id)
            .unwrap();
        system_meta
            .archetype_component_access
            .add_write(archetype_component_id);
        Self {
            component_id,
            marker: PhantomData,
        }
    }
}

impl<'w, 's, T: Resource> SystemParamFetch<'w, 's> for ResMutState<T> {
    type Item = ResMut<'w, T>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        change_tick: u32,
    ) -> Self::Item {
        let value = world
            .as_ref()
            .get_resource_unchecked_mut_with_id(state.component_id)
            .unwrap_or_else(|| {
                panic!(
                    "Resource requested by {} does not exist: {}",
                    system_meta.name,
                    std::any::type_name::<T>()
                )
            });
        ResMut {
            value: value.value,
            ticks: Ticks {
                component_ticks: value.ticks.component_ticks,
                last_change_tick: system_meta.last_change_tick,
                change_tick,
            },
        }
    }
}

/// The [`SystemParamState`] of [`Option<ResMut<T>>`](`ResMut`).
#[doc(hidden)]
pub struct OptionResMutState<T>(ResMutState<T>);

impl<'a, T: Resource> SystemParam for Option<ResMut<'a, T>> {
    type Fetch = OptionResMutState<T>;
}

// SAFETY: See ResMut<T>
unsafe impl<T: Resource> SystemParamState for OptionResMutState<T> {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        Self(ResMutState::init(world, system_meta))
    }
}

impl<'w, 's, T: Resource> SystemParamFetch<'w, 's> for OptionResMutState<T> {
    type Item = Option<ResMut<'w, T>>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        change_tick: u32,
    ) -> Self::Item {
        world
            .as_ref()
            .get_resource_unchecked_mut_with_id(state.0.component_id)
            .map(|value| ResMut {
                value: value.value,
                ticks: Ticks {
                    component_ticks: value.ticks.component_ticks,
                    last_change_tick: system_meta.last_change_tick,
                    change_tick,
                },
            })
    }
}

impl<'w, 's> SystemParam for Commands<'w, 's> {
    type Fetch = CommandQueue;
}

// SAFETY: commands only access internal state and &Entities (which impl ReadOnlySystemParamFetch)
unsafe impl ReadOnlySystemParamFetch for CommandQueue {}

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl SystemParamState for CommandQueue {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        add_shared_world_access(world, system_meta, false, "Commands");
        Default::default()
    }

    fn apply(&mut self, world: &mut World) {
        self.apply(world);
    }
}

impl<'w, 's> SystemParamFetch<'w, 's> for CommandQueue {
    type Item = Commands<'w, 's>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        _system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        _change_tick: u32,
    ) -> Self::Item {
        Commands::new(state, world.as_ref())
    }
}

/// The [`SystemParamState`] of [`&World`](crate::world::World).
#[doc(hidden)]
pub struct WorldState;

impl<'w, 's> SystemParam for &'w World {
    type Fetch = WorldState;
}

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl<'w, 's> SystemParamState for WorldState {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        add_shared_world_access(world, system_meta, true, "&World");

        WorldState
    }
}

impl<'w, 's> SystemParamFetch<'w, 's> for WorldState {
    type Item = &'w World;
    unsafe fn get_param(
        _state: &'s mut Self,
        _system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        _change_tick: u32,
    ) -> Self::Item {
        world.as_ref()
    }
}

/// SAFETY: &World is an non-mutable borrow.
unsafe impl ReadOnlySystemParamFetch for WorldState {}

/// The [`SystemParamState`] of [`&mut World`](crate::world::World).
#[doc(hidden)]
pub struct WorldMutState;

impl<'w, 's> SystemParam for &'w mut World {
    type Fetch = WorldMutState;
}

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl<'w, 's> SystemParamState for WorldMutState {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        // world could contain non-send resources, run on local thread
        system_meta.set_non_send();
        add_exclusive_world_access(world, system_meta, "&mut World");

        WorldMutState
    }
}

impl<'w, 's> SystemParamFetch<'w, 's> for WorldMutState {
    type Item = &'w mut World;
    unsafe fn get_param(
        _state: &'s mut Self,
        _system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        _change_tick: u32,
    ) -> Self::Item {
        world.as_mut()
    }
}

/// A [`SystemParam`] that is stored on the system itself.
///
/// A `Local` cannot be read or written to from outside its containing system.
/// If several systems have the same local type, they will all have their own unique instance.
///
/// # Examples
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # let world = &mut World::default();
/// fn write_to_local(mut local: Local<usize>) {
///     *local = 42;
/// }
/// fn read_from_local(local: Local<usize>) -> usize {
///     *local
/// }
/// let mut write_system = IntoSystem::into_system(write_to_local);
/// let mut read_system = IntoSystem::into_system(read_from_local);
/// write_system.initialize(world);
/// read_system.initialize(world);
///
/// assert_eq!(read_system.run((), world), 0);
/// write_system.run((), world);
/// // Note how the read local is still 0 due to the locals not being shared.
/// assert_eq!(read_system.run((), world), 0);
/// ```
///
/// **Tip:** You can convert a capturing closure into a system to configure it.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::system::assert_is_system;
/// struct Config(u32);
/// struct Myu32Wrapper(u32);
/// fn reset_to_system(value: Config) -> impl FnMut(ResMut<Myu32Wrapper>) {
///     move |mut val| val.0 = value.0
/// }
///
/// // .add_system(reset_to_system(my_config))
/// # assert_is_system(reset_to_system(Config(10)));
/// ```
pub struct Local<'a, T: Resource>(&'a mut T);

impl<'a, T: Resource> Debug for Local<'a, T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Local").field(&self.0).finish()
    }
}

impl<'a, T: Resource> Deref for Local<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'a, T: Resource> DerefMut for Local<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

// SAFETY: only reads internal system data
unsafe impl<T: Resource> ReadOnlySystemParamFetch for LocalState<T> {}

/// The [`SystemParamState`] of [`Local<T>`].
#[doc(hidden)]
pub struct LocalState<T: Resource>(T);

impl<'a, T: Resource + FromWorld> SystemParam for Local<'a, T> {
    type Fetch = LocalState<T>;
}

// SAFETY: only local state is accessed
unsafe impl<T: Resource + FromWorld> SystemParamState for LocalState<T> {
    fn init(world: &mut World, _system_meta: &mut SystemMeta) -> Self {
        Self(T::from_world(world))
    }
}

impl<'w, 's, T: Resource + FromWorld> SystemParamFetch<'w, 's> for LocalState<T> {
    type Item = Local<'s, T>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        _system_meta: &SystemMeta,
        _world: SemiSafeCell<'w, World>,
        _change_tick: u32,
    ) -> Self::Item {
        Local(&mut state.0)
    }
}

/// A [`SystemParam`] that iterates entities whose `T` [`Component`] was removed.
///
/// This param does *not* return the data that was removed (that's gone), but it
/// may return entities that have already been despawned.
///
/// - If you are using `bevy_ecs` as a standalone crate, `RemovedComponents` will need to be
/// manually cleared using [`World::clear_trackers`].
/// - If you are using `bevy`, both the `MinimalPlugins` and `DefaultPlugins` add [`World::clear_trackers`]
/// as a system to [`CoreSet::Last`](bevy_core::CoreSet::Last).
///
/// # Examples
///
/// Basic usage:
///
/// ```
/// # use bevy_ecs::component::Component;
/// # use bevy_ecs::system::IntoSystem;
/// # use bevy_ecs::system::RemovedComponents;
/// #
/// # #[derive(Component)]
/// # struct MyComponent;
///
/// fn react_on_removal(removed: RemovedComponents<MyComponent>) {
///     removed.iter().for_each(|removed_entity| println!("{:?}", removed_entity));
/// }
///
/// # bevy_ecs::system::assert_is_system(react_on_removal);
/// ```
pub struct RemovedComponents<'a, T: Component> {
    world: &'a World,
    component_id: ComponentId,
    marker: PhantomData<T>,
}

impl<'a, T: Component> RemovedComponents<'a, T> {
    /// Returns an iterator over the entities that had their `T` [`Component`] removed.
    pub fn iter(&self) -> std::iter::Cloned<std::slice::Iter<'_, Entity>> {
        self.world.removed_with_id(self.component_id)
    }
}

/// The [`SystemParamState`] of [`RemovedComponents<T>`].
#[doc(hidden)]
pub struct RemovedComponentsState<T> {
    component_id: ComponentId,
    marker: PhantomData<T>,
}

impl<'a, T: Component> SystemParam for RemovedComponents<'a, T> {
    type Fetch = RemovedComponentsState<T>;
}

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl<T: Component> SystemParamState for RemovedComponentsState<T> {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        let param_name = format!("RemovedComponents<{}>", std::any::type_name::<T>());
        add_shared_world_access(world, system_meta, false, param_name.as_ref());

        Self {
            component_id: world.init_component::<T>(),
            marker: PhantomData,
        }
    }
}

impl<'w, 's, T: Component> SystemParamFetch<'w, 's> for RemovedComponentsState<T> {
    type Item = RemovedComponents<'w, T>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        _system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        _change_tick: u32,
    ) -> Self::Item {
        RemovedComponents {
            world: world.as_ref(),
            component_id: state.component_id,
            marker: PhantomData,
        }
    }
}

// SAFETY: non-mutable borrow
unsafe impl<T: Component> ReadOnlySystemParamFetch for RemovedComponentsState<T> {}

/// Shared borrow of a non-[`Send`] resource.
///
/// Only `Send` resources may be accessed with [`Res<T>`]. If a resource does not implement `Send`,
/// this [`SystemParam`](crate::system::SystemParam) must be used to ensure the accessing system runs on the same thread.
///
/// For a unique, mutable borrow, see [`NonSendMut<T>`](NonSendMut).
///
/// # Panics
///
/// Panics when used as a `SystemParam` if `T` has not be inserted as a resource.
///
/// Use `Option<NonSend<T>>` instead if the resource might not always exist.
pub struct NonSend<'w, T: 'static> {
    pub(crate) value: &'w T,
    ticks: ComponentTicks,
    last_change_tick: u32,
    change_tick: u32,
}

impl<'w, T> Debug for NonSend<'w, T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("NonSend").field(&self.value).finish()
    }
}

impl<'w, T: 'static> NonSend<'w, T> {
    /// Returns `true` if the resource was added after the system last ran.
    pub fn is_added(&self) -> bool {
        self.ticks.is_added(self.last_change_tick, self.change_tick)
    }

    /// Returns `true` if the resource was added or mutably-dereferenced after the system last ran.
    pub fn is_changed(&self) -> bool {
        self.ticks
            .is_changed(self.last_change_tick, self.change_tick)
    }
}

impl<'w, T> Deref for NonSend<'w, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}
impl<'a, T> From<NonSendMut<'a, T>> for NonSend<'a, T> {
    fn from(nsm: NonSendMut<'a, T>) -> Self {
        Self {
            value: nsm.value,
            ticks: nsm.ticks.component_ticks.to_owned(),
            change_tick: nsm.ticks.change_tick,
            last_change_tick: nsm.ticks.last_change_tick,
        }
    }
}

/// The [`SystemParamState`] of [`NonSend<T>`].
#[doc(hidden)]
pub struct NonSendState<T> {
    component_id: ComponentId,
    marker: PhantomData<fn() -> T>,
}

impl<'a, T: 'static> SystemParam for NonSend<'a, T> {
    type Fetch = NonSendState<T>;
}

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl<T: 'static> SystemParamState for NonSendState<T> {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        system_meta.set_non_send();

        let component_id = world.initialize_non_send_resource::<T>();
        let combined_access = system_meta.component_access_set.combined_access_mut();
        assert!(
            !combined_access.has_write(component_id),
            "error[B0002]: NonSend<{}> in system {} conflicts with a previous mutable resource access ({0}). Consider removing the duplicate access.",
            std::any::type_name::<T>(),
            system_meta.name,
        );
        combined_access.add_read(component_id);

        let resource_archetype = world.archetypes.resource();
        let archetype_component_id = resource_archetype
            .get_archetype_component_id(component_id)
            .unwrap();
        system_meta
            .archetype_component_access
            .add_read(archetype_component_id);
        Self {
            component_id,
            marker: PhantomData,
        }
    }
}

impl<'w, 's, T: 'static> SystemParamFetch<'w, 's> for NonSendState<T> {
    type Item = NonSend<'w, T>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        change_tick: u32,
    ) -> Self::Item {
        let world = world.as_ref();
        world.validate_non_send_access::<T>();
        let column = world
            .get_populated_resource_column(state.component_id)
            .unwrap_or_else(|| {
                panic!(
                    "Non-send resource requested by {} does not exist: {}",
                    system_meta.name,
                    std::any::type_name::<T>()
                )
            });

        NonSend {
            value: &*column.get_data_ptr().cast::<T>().as_ptr(),
            ticks: column.get_ticks_unchecked(0).clone(),
            last_change_tick: system_meta.last_change_tick,
            change_tick,
        }
    }
}

// SAFETY: non-mutable borrow (also can only be accessed on one thread)
unsafe impl<T: 'static> ReadOnlySystemParamFetch for NonSendState<T> {}

/// The [`SystemParamState`] of [`Option<NonSend<T>>`](`NonSend`).
#[doc(hidden)]
pub struct OptionNonSendState<T>(NonSendState<T>);

impl<'w, T: 'static> SystemParam for Option<NonSend<'w, T>> {
    type Fetch = OptionNonSendState<T>;
}

// SAFETY: See NonSendState<T>
unsafe impl<T: 'static> SystemParamState for OptionNonSendState<T> {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        Self(NonSendState::init(world, system_meta))
    }
}

impl<'w, 's, T: 'static> SystemParamFetch<'w, 's> for OptionNonSendState<T> {
    type Item = Option<NonSend<'w, T>>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        change_tick: u32,
    ) -> Self::Item {
        let world = world.as_ref();
        world.validate_non_send_access::<T>();
        world
            .get_populated_resource_column(state.0.component_id)
            .map(|column| NonSend {
                value: &*column.get_data_ptr().cast::<T>().as_ptr(),
                ticks: column.get_ticks_unchecked(0).clone(),
                last_change_tick: system_meta.last_change_tick,
                change_tick,
            })
    }
}

// SAFETY: non-mutable borrow (also can only be accessed on one thread)
unsafe impl<T: 'static> ReadOnlySystemParamFetch for OptionNonSendState<T> {}

/// The [`SystemParamState`] of [`NonSendMut<T>`].
#[doc(hidden)]
pub struct NonSendMutState<T> {
    component_id: ComponentId,
    marker: PhantomData<fn() -> T>,
}

impl<'a, T: 'static> SystemParam for NonSendMut<'a, T> {
    type Fetch = NonSendMutState<T>;
}

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl<T: 'static> SystemParamState for NonSendMutState<T> {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        system_meta.set_non_send();

        let component_id = world.initialize_non_send_resource::<T>();
        let combined_access = system_meta.component_access_set.combined_access_mut();
        if combined_access.has_write(component_id) {
            panic!(
                "error[B0002]: NonSendMut<{}> in system {} conflicts with a previous mutable resource access ({0}). Consider removing the duplicate access.",
                std::any::type_name::<T>(), system_meta.name);
        } else if combined_access.has_read(component_id) {
            panic!(
                "error[B0002]: NonSendMut<{}> in system {} conflicts with a previous immutable resource access ({0}). Consider removing the duplicate access.",
                std::any::type_name::<T>(), system_meta.name);
        }
        combined_access.add_write(component_id);

        let resource_archetype = world.archetypes.resource();
        let archetype_component_id = resource_archetype
            .get_archetype_component_id(component_id)
            .unwrap();
        system_meta
            .archetype_component_access
            .add_write(archetype_component_id);
        Self {
            component_id,
            marker: PhantomData,
        }
    }
}

impl<'w, 's, T: 'static> SystemParamFetch<'w, 's> for NonSendMutState<T> {
    type Item = NonSendMut<'w, T>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        change_tick: u32,
    ) -> Self::Item {
        let world = world.as_ref();
        world.validate_non_send_access::<T>();
        let column = world
            .get_populated_resource_column(state.component_id)
            .unwrap_or_else(|| {
                panic!(
                    "Non-send resource requested by {} does not exist: {}",
                    system_meta.name,
                    std::any::type_name::<T>()
                )
            });
        NonSendMut {
            value: &mut *column.get_data_ptr().cast::<T>().as_ptr(),
            ticks: Ticks {
                component_ticks: &mut *column.get_ticks_mut_ptr_unchecked(0),
                last_change_tick: system_meta.last_change_tick,
                change_tick,
            },
        }
    }
}

/// The [`SystemParamState`] of [`Option<NonSendMut<T>>`](`NonSendMut`).
#[doc(hidden)]
pub struct OptionNonSendMutState<T>(NonSendMutState<T>);

impl<'a, T: 'static> SystemParam for Option<NonSendMut<'a, T>> {
    type Fetch = OptionNonSendMutState<T>;
}

// SAFETY: See NonSendMut<T>
unsafe impl<T: 'static> SystemParamState for OptionNonSendMutState<T> {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        Self(NonSendMutState::init(world, system_meta))
    }
}

impl<'w, 's, T: 'static> SystemParamFetch<'w, 's> for OptionNonSendMutState<T> {
    type Item = Option<NonSendMut<'w, T>>;

    #[inline]
    unsafe fn get_param(
        state: &'s mut Self,
        system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        change_tick: u32,
    ) -> Self::Item {
        let world = world.as_ref();
        world.validate_non_send_access::<T>();
        world
            .get_populated_resource_column(state.0.component_id)
            .map(|column| NonSendMut {
                value: &mut *column.get_data_ptr().cast::<T>().as_ptr(),
                ticks: Ticks {
                    component_ticks: &mut *column.get_ticks_mut_ptr_unchecked(0),
                    last_change_tick: system_meta.last_change_tick,
                    change_tick,
                },
            })
    }
}

impl<'a> SystemParam for &'a Archetypes {
    type Fetch = ArchetypesState;
}

/// The [`SystemParamState`] of [`&Archetypes`](Archetypes).
#[doc(hidden)]
pub struct ArchetypesState;

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl SystemParamState for ArchetypesState {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        add_shared_world_access(world, system_meta, false, "&Archetypes");

        Self
    }
}

impl<'w, 's> SystemParamFetch<'w, 's> for ArchetypesState {
    type Item = &'w Archetypes;

    #[inline]
    unsafe fn get_param(
        _state: &'s mut Self,
        _system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        _change_tick: u32,
    ) -> Self::Item {
        world.as_ref().archetypes()
    }
}

// SAFETY: &Archetypes is an non-mutable borrow
unsafe impl ReadOnlySystemParamFetch for ArchetypesState {}

impl<'a> SystemParam for &'a Components {
    type Fetch = ComponentsState;
}

/// The [`SystemParamState`] of [`&Components`](Components).
#[doc(hidden)]
pub struct ComponentsState;

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl SystemParamState for ComponentsState {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        add_shared_world_access(world, system_meta, false, "&Components");

        Self
    }
}

impl<'w, 's> SystemParamFetch<'w, 's> for ComponentsState {
    type Item = &'w Components;

    #[inline]
    unsafe fn get_param(
        _state: &'s mut Self,
        _system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        _change_tick: u32,
    ) -> Self::Item {
        world.as_ref().components()
    }
}

impl<'a> SystemParam for &'a Entities {
    type Fetch = EntitiesState;
}

// SAFETY: &Components is an non-mutable borrow
unsafe impl ReadOnlySystemParamFetch for ComponentsState {}

/// The [`SystemParamState`] of [`&Entities`](Entities).
#[doc(hidden)]
pub struct EntitiesState;

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl SystemParamState for EntitiesState {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        add_shared_world_access(world, system_meta, false, "&Entities");

        Self
    }
}

impl<'w, 's> SystemParamFetch<'w, 's> for EntitiesState {
    type Item = &'w Entities;

    #[inline]
    unsafe fn get_param(
        _state: &'s mut Self,
        _system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        _change_tick: u32,
    ) -> Self::Item {
        world.as_ref().entities()
    }
}

impl<'a> SystemParam for &'a Bundles {
    type Fetch = BundlesState;
}

// SAFETY: &Entities is a thread-safe shared borrow
unsafe impl ReadOnlySystemParamFetch for EntitiesState {}

/// The [`SystemParamState`] of [`&Bundles`](Bundles).
#[doc(hidden)]
pub struct BundlesState;

// SAFETY: ComponentId and ArchetypeComponentId access is checked against the SystemMeta.
// This will panic if there's a conflict with any prior access.
unsafe impl SystemParamState for BundlesState {
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        add_shared_world_access(world, system_meta, false, "&Bundles");

        Self
    }
}

impl<'w, 's> SystemParamFetch<'w, 's> for BundlesState {
    type Item = &'w Bundles;

    #[inline]
    unsafe fn get_param(
        _state: &'s mut Self,
        _system_meta: &SystemMeta,
        world: SemiSafeCell<'w, World>,
        _change_tick: u32,
    ) -> Self::Item {
        world.as_ref().bundles()
    }
}

// SAFETY: &Bundles is an non-mutable borrow
unsafe impl ReadOnlySystemParamFetch for BundlesState {}

#[derive(Debug)]
pub struct SystemChangeTick {
    pub last_change_tick: u32,
    pub change_tick: u32,
}

impl SystemParam for SystemChangeTick {
    type Fetch = SystemChangeTickState;
}

/// The [`SystemParamState`] of [`SystemChangeTick`].
#[doc(hidden)]
pub struct SystemChangeTickState {}

// SAFETY: only reads internal system data
unsafe impl SystemParamState for SystemChangeTickState {
    fn init(_world: &mut World, _system_meta: &mut SystemMeta) -> Self {
        Self {}
    }
}

impl<'w, 's> SystemParamFetch<'w, 's> for SystemChangeTickState {
    type Item = SystemChangeTick;

    unsafe fn get_param(
        _state: &'s mut Self,
        system_meta: &SystemMeta,
        _world: SemiSafeCell<'w, World>,
        change_tick: u32,
    ) -> Self::Item {
        SystemChangeTick {
            last_change_tick: system_meta.last_change_tick,
            change_tick,
        }
    }
}

// SAFETY: only reads internal system data
unsafe impl ReadOnlySystemParamFetch for SystemChangeTickState {}

macro_rules! impl_system_param_tuple {
    ($($param: ident),*) => {
        impl<$($param: SystemParam),*> SystemParam for ($($param,)*) {
            type Fetch = ($($param::Fetch,)*);
        }

        // SAFETY: tuple consists only of types that impl ReadOnlySystemParamFetch
        unsafe impl<$($param: ReadOnlySystemParamFetch),*> ReadOnlySystemParamFetch for ($($param,)*) {}

        #[allow(unused_variables)]
        #[allow(non_snake_case)]
        impl<'w, 's, $($param: SystemParamFetch<'w, 's>),*> SystemParamFetch<'w, 's> for ($($param,)*) {
            type Item = ($($param::Item,)*);

            #[inline]
            #[allow(clippy::unused_unit)]
            unsafe fn get_param(
                state: &'s mut Self,
                system_meta: &SystemMeta,
                world: SemiSafeCell<'w, World>,
                change_tick: u32,
            ) -> Self::Item {

                let ($($param,)*) = state;
                ($($param::get_param($param, system_meta, world, change_tick),)*)
            }
        }

        // SAFETY: implementors assure their `SystemParamState` impls follow the rules
        #[allow(non_snake_case)]
        unsafe impl<$($param: SystemParamState),*> SystemParamState for ($($param,)*) {
            #[inline]
            fn init(_world: &mut World, _system_meta: &mut SystemMeta) -> Self {
                (($($param::init(_world, _system_meta),)*))
            }

            #[inline]
            fn new_archetype(&mut self, _archetype: &Archetype, _system_meta: &mut SystemMeta) {
                let ($($param,)*) = self;
                $($param.new_archetype(_archetype, _system_meta);)*
            }

            #[inline]
            fn apply(&mut self, _world: &mut World) {
                let ($($param,)*) = self;
                $($param.apply(_world);)*
            }
        }
    };
}

all_tuples!(impl_system_param_tuple, 0, 16, P);

pub mod lifetimeless {
    pub type SQuery<Q, F = ()> = super::Query<'static, 'static, Q, F>;
    pub type Read<T> = &'static T;
    pub type Write<T> = &'static mut T;
    pub type SRes<T> = super::Res<'static, T>;
    pub type SResMut<T> = super::ResMut<'static, T>;
    pub type SCommands = crate::system::Commands<'static, 'static>;
}

/// A helper for using system parameters in generic contexts
///
/// This type is a [`SystemParam`] adapter which always has
/// `Self::Fetch::Item == Self` (ignoring lifetimes for brevity),
/// no matter the argument [`SystemParam`] (`P`) (other than
/// that `P` must be `'static`)
///
/// This makes it useful for having arbitrary [`SystemParam`] type arguments
/// to function systems, or for generic types using the [`derive@SystemParam`]
/// derive:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// use bevy_ecs::system::{SystemParam, StaticSystemParam};
/// #[derive(SystemParam)]
/// struct GenericParam<'w,'s, T: SystemParam + 'static> {
///     field: StaticSystemParam<'w, 's, T>,
/// }
/// fn do_thing_generically<T: SystemParam + 'static>(t: StaticSystemParam<T>) {}
///
/// fn check_always_is_system<T: SystemParam + 'static>(){
///     bevy_ecs::system::assert_is_system(do_thing_generically::<T>);
/// }
/// ```
/// Note that in a real case you'd generally want
/// additional bounds on `P`, for your use of the parameter
/// to have a reason to be generic.
///
/// For example, using this would allow a type to be generic over
/// whether a resource is accessed mutably or not, with
/// impls being bounded on [`P: Deref<Target=MyType>`](Deref), and
/// [`P: DerefMut<Target=MyType>`](DerefMut) depending on whether the
/// method requires mutable access or not.
///
/// The method which doesn't use this type will not compile:
/// ```compile_fail
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::system::{SystemParam, StaticSystemParam};
///
/// fn do_thing_generically<T: SystemParam + 'static>(t: T) {}
///
/// #[derive(SystemParam)]
/// struct GenericParam<'w,'s, T: SystemParam> {
///     field: T,
///     #[system_param(ignore)]
///     // Use the lifetimes, as the `SystemParam` derive requires them
///     phantom: core::marker::PhantomData<&'w &'s ()>
/// }
/// # fn check_always_is_system<T: SystemParam + 'static>(){
/// #    bevy_ecs::system::assert_is_system(do_thing_generically::<T>);
/// # }
/// ```
///
pub struct StaticSystemParam<'w, 's, P: SystemParam>(SystemParamItem<'w, 's, P>);

impl<'w, 's, P: SystemParam> Deref for StaticSystemParam<'w, 's, P> {
    type Target = SystemParamItem<'w, 's, P>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'w, 's, P: SystemParam> DerefMut for StaticSystemParam<'w, 's, P> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'w, 's, P: SystemParam> StaticSystemParam<'w, 's, P> {
    /// Get the value of the parameter
    pub fn into_inner(self) -> SystemParamItem<'w, 's, P> {
        self.0
    }
}

/// The [`SystemParamState`] of [`StaticSystemParam`].
#[doc(hidden)]
pub struct StaticSystemParamState<S, P>(S, PhantomData<fn() -> P>);

// Safe: This doesn't add any more reads, and the delegated fetch confirms it
unsafe impl<'w, 's, S: ReadOnlySystemParamFetch, P> ReadOnlySystemParamFetch
    for StaticSystemParamState<S, P>
{
}

impl<'world, 'state, P: SystemParam + 'static> SystemParam
    for StaticSystemParam<'world, 'state, P>
{
    type Fetch = StaticSystemParamState<P::Fetch, P>;
}

impl<'world, 'state, S: SystemParamFetch<'world, 'state>, P: SystemParam + 'static>
    SystemParamFetch<'world, 'state> for StaticSystemParamState<S, P>
where
    P: SystemParam<Fetch = S>,
{
    type Item = StaticSystemParam<'world, 'state, P>;

    unsafe fn get_param(
        state: &'state mut Self,
        system_meta: &SystemMeta,
        world: SemiSafeCell<'world, World>,
        change_tick: u32,
    ) -> Self::Item {
        // Safe: We properly delegate SystemParamState
        StaticSystemParam(S::get_param(&mut state.0, system_meta, world, change_tick))
    }
}

unsafe impl<'w, 's, S: SystemParamState, P: SystemParam + 'static> SystemParamState
    for StaticSystemParamState<S, P>
{
    fn init(world: &mut World, system_meta: &mut SystemMeta) -> Self {
        Self(S::init(world, system_meta), PhantomData)
    }

    fn new_archetype(&mut self, archetype: &Archetype, system_meta: &mut SystemMeta) {
        self.0.new_archetype(archetype, system_meta)
    }

    fn apply(&mut self, world: &mut World) {
        self.0.apply(world)
    }
}

#[cfg(test)]
mod tests {
    use super::SystemParam;
    use crate::{
        self as bevy_ecs, // Necessary for the `SystemParam` Derive when used inside `bevy_ecs`.
        query::{FilterFetch, WorldQuery},
        system::Query,
    };

    // Compile test for #2838
    #[derive(SystemParam)]
    pub struct SpecialQuery<
        'w,
        's,
        Q: WorldQuery + Send + Sync + 'static,
        F: WorldQuery + Send + Sync + 'static = (),
    >
    where
        F::Fetch: FilterFetch,
    {
        _query: Query<'w, 's, Q, F>,
    }
}
