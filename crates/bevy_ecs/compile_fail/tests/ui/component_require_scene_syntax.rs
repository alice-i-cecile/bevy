use bevy_ecs::prelude::*;

#[derive(Component, Default, Clone)]
struct A;

#[derive(Component)]
//~v ERROR: Names are not supported in `#[require]`
#[require(#Player)]
struct Named;

//~v ERROR: Scenes are not supported in `#[require]`
#[derive(Component)]
#[require(@A)]
struct Scene;

#[derive(Component)]
//~v ERROR: Related scene lists are not supported in `#[require]`
#[require(Children [A])]
struct Related;
