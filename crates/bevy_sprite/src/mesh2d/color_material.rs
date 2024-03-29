use crate::{Material2d, Material2dPlugin, MaterialMesh2dBundle};
use bevy_app::{App, Plugin};
use bevy_asset::{load_internal_asset, Asset, AssetApp, Assets, Handle};
use bevy_color::{Hsla, Hsva, LinearRgba, Oklaba, Oklcha, Srgba};
use bevy_math::Vec4;
use bevy_reflect::prelude::*;
use bevy_render::{render_asset::RenderAssets, render_resource::*, texture::Image};

pub const COLOR_MATERIAL_SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(3253086872234592509);

#[derive(Default)]
pub struct ColorMaterialPlugin;

impl Plugin for ColorMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            COLOR_MATERIAL_SHADER_HANDLE,
            "color_material.wgsl",
            Shader::from_wgsl
        );

        app.add_plugins(Material2dPlugin::<ColorMaterial>::default())
            .register_asset_reflect::<ColorMaterial>();

        app.world.resource_mut::<Assets<ColorMaterial>>().insert(
            &Handle::<ColorMaterial>::default(),
            ColorMaterial {
                color: LinearRgba::WHITE,
                ..Default::default()
            },
        );
    }
}

/// A [2d material](Material2d) that renders [2d meshes](crate::Mesh2dHandle) with a texture tinted by a uniform color
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[reflect(Default, Debug)]
#[uniform(0, ColorMaterialUniform)]
pub struct ColorMaterial {
    pub color: LinearRgba,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Option<Handle<Image>>,
}

impl Default for ColorMaterial {
    fn default() -> Self {
        ColorMaterial {
            color: LinearRgba::WHITE,
            texture: None,
        }
    }
}

impl From<LinearRgba> for ColorMaterial {
    fn from(color: LinearRgba) -> Self {
        ColorMaterial {
            color,
            ..Default::default()
        }
    }
}

impl From<Srgba> for ColorMaterial {
    fn from(color: Srgba) -> Self {
        ColorMaterial {
            color: color.into(),
            ..Default::default()
        }
    }
}

impl From<Hsla> for ColorMaterial {
    fn from(color: Hsla) -> Self {
        ColorMaterial {
            color: color.into(),
            ..Default::default()
        }
    }
}

impl From<Hsva> for ColorMaterial {
    fn from(color: Hsva) -> Self {
        ColorMaterial {
            color: color.into(),
            ..Default::default()
        }
    }
}

impl From<Oklaba> for ColorMaterial {
    fn from(color: Oklaba) -> Self {
        ColorMaterial {
            color: color.into(),
            ..Default::default()
        }
    }
}

impl From<Oklcha> for ColorMaterial {
    fn from(color: Oklcha) -> Self {
        ColorMaterial {
            color: color.into(),
            ..Default::default()
        }
    }
}

impl From<Handle<Image>> for ColorMaterial {
    fn from(texture: Handle<Image>) -> Self {
        ColorMaterial {
            texture: Some(texture),
            ..Default::default()
        }
    }
}

// NOTE: These must match the bit flags in bevy_sprite/src/mesh2d/color_material.wgsl!
bitflags::bitflags! {
    #[repr(transparent)]
    pub struct ColorMaterialFlags: u32 {
        const TEXTURE           = 1 << 0;
        const NONE              = 0;
        const UNINITIALIZED     = 0xFFFF;
    }
}

/// The GPU representation of the uniform data of a [`ColorMaterial`].
#[derive(Clone, Default, ShaderType)]
pub struct ColorMaterialUniform {
    pub color: Vec4,
    pub flags: u32,
}

impl AsBindGroupShaderType<ColorMaterialUniform> for ColorMaterial {
    fn as_bind_group_shader_type(&self, _images: &RenderAssets<Image>) -> ColorMaterialUniform {
        let mut flags = ColorMaterialFlags::NONE;
        if self.texture.is_some() {
            flags |= ColorMaterialFlags::TEXTURE;
        }

        ColorMaterialUniform {
            color: LinearRgba::from(self.color).to_f32_array().into(),
            flags: flags.bits(),
        }
    }
}

impl Material2d for ColorMaterial {
    fn fragment_shader() -> ShaderRef {
        COLOR_MATERIAL_SHADER_HANDLE.into()
    }
}

/// A component bundle for entities with a [`Mesh2dHandle`](crate::Mesh2dHandle) and a [`ColorMaterial`].
pub type ColorMesh2dBundle = MaterialMesh2dBundle<ColorMaterial>;
