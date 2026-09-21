use bevy_asset::{AssetServer, Assets};
use bevy_ecs::{
    component::Component,
    error::{BevyError, Result},
    reflect::ReflectComponent,
    template::{FromTemplate, Template, TemplateContext},
};
use bevy_reflect::Reflect;
use core::{any::TypeId, marker::PhantomData};

use crate::{ResolvedSceneRoot, Scene, ScenePatch};

/// Implemented for [`Component`]s that have an associated [`Scene`], which can be constructed
/// with [`Self::Props`].
///
/// In general, developers should not implement this manually. Instead, they should derive it,
/// which also derives [`Component`] and adds additional protections and assurances.
///
/// See the ["Scene Components"](crate#scene-components) section of the module docs to see how this is used in practice.
pub trait SceneComponent: Component + FromTemplate<Template: Default> {
    /// The "properties" passed into [`Self::scene`] to build the final scene.
    type Props: Default;

    /// A function that uses the given `props` to produce a [`Scene`]
    fn scene(props: Self::Props) -> impl Scene;
}

/// Indicates that this entity includes a [`Component`] that has been spawned with its [`Scene`].
///
/// Scene components require this component using [`ApplySceneComponent`], so inserting a scene
/// component outside of a scene (ex: with [`World::spawn`](bevy_ecs::world::World::spawn)) applies its scene.
#[derive(Component, Default, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct SceneComponentInfo {
    spawned_from_scene: bool,
    #[cfg(debug_assertions)]
    component_name: &'static str,
}

impl SceneComponentInfo {
    /// Creates a new [`SceneComponentInfo`] for the given type `C`.
    pub fn new<C: Component>(spawned_from_scene: bool) -> Self {
        SceneComponentInfo {
            spawned_from_scene,
            #[cfg(debug_assertions)]
            component_name: core::any::type_name::<C>(),
        }
    }
}

/// A [`Template`] that applies the [`Scene`] of the [`SceneComponent`] `C` to the entity it is built for,
/// using the default [`SceneComponent::Props`].
///
/// This is used as the required [`SceneComponentInfo`] of every scene component. When a scene component
/// is spawned as a scene, [`SceneComponentInfo`] is already part of the scene and this does nothing.
/// When it is inserted like any other component, this applies its scene first.
///
/// The scene's value for `C` itself is skipped: the inserted `C` always wins, just like any other
/// explicitly inserted component wins over a required one.
pub struct ApplySceneComponent<C>(PhantomData<fn() -> C>);

impl<C> Default for ApplySceneComponent<C> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<C: SceneComponent> Template for ApplySceneComponent<C> {
    type Output = SceneComponentInfo;

    fn build_template(&self, context: &mut TemplateContext) -> Result<SceneComponentInfo> {
        let mut resolved = {
            let world = context.entity.world();
            let (Some(assets), Some(patches)) = (
                world.get_resource::<AssetServer>(),
                world.get_resource::<Assets<ScenePatch>>(),
            ) else {
                return Err(BevyError::error(
                    "Scene components can only be inserted into worlds with the ScenePlugin",
                ));
            };
            ResolvedSceneRoot::resolve(Box::new(C::scene(C::Props::default())), assets, patches)?
        };
        resolved.scene.remove_template(TypeId::of::<C::Template>());
        resolved
            .scene
            .remove_template(TypeId::of::<SceneComponentInfo>());
        resolved.apply(context.entity, &mut Default::default())?;
        Ok(SceneComponentInfo::new::<C>(true))
    }

    fn clone_template(&self) -> Self {
        Self::default()
    }
}
