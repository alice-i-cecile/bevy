//! This example illustrates the various features of Bevy UI.

use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    winit::WinitSettings,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // Only run the app when there is user input. This will significantly reduce CPU/GPU use.
        .insert_resource(WinitSettings::desktop_app())
        .add_startup_system(setup)
        .add_system(mouse_scroll)
        .run();
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Camera
    commands.spawn_bundle(Camera2dBundle::default());

    // root node
    commands
        .spawn_bundle(NodeBundle {
            size_constraints: SizeConstraints::FULL,
            flex_layout: FlexLayout {
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            color: Color::NONE.into(),
            ..default()
        })
        .with_children(|parent| {
            // left vertical fill (border)
            parent
                .spawn_bundle(NodeBundle {
                    size_constraints: SizeConstraints::suggested(
                        Val::Px(200.0),
                        Val::Percent(100.0),
                    ),
                    spacing: Spacing::border_all(Val::Px(2.0)),
                    color: Color::rgb(0.65, 0.65, 0.65).into(),
                    ..default()
                })
                .with_children(|parent| {
                    // left vertical fill (content)
                    parent
                        .spawn_bundle(NodeBundle {
                            size_constraints: SizeConstraints::FULL,
                            flex_layout: FlexLayout {
                                align_items: AlignItems::FlexEnd,
                                ..default()
                            },
                            color: Color::rgb(0.15, 0.15, 0.15).into(),
                            ..default()
                        })
                        .with_children(|parent| {
                            // text
                            parent
                                .spawn_bundle(TextBundle::from_section(
                                    "Text Example",
                                    TextStyle {
                                        font: asset_server.load("fonts/FiraSans-Bold.ttf"),
                                        font_size: 30.0,
                                        color: Color::WHITE,
                                    },
                                ))
                                .insert(Spacing::margin_all(Val::Px(5.0)));
                        });
                });
            // right vertical fill
            parent
                .spawn_bundle(NodeBundle {
                    size_constraints: SizeConstraints::suggested(
                        Val::Px(200.0),
                        Val::Percent(100.0),
                    ),
                    flex_layout: FlexLayout {
                        flex_direction: FlexDirection::ColumnReverse,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    color: Color::rgb(0.15, 0.15, 0.15).into(),
                    ..default()
                })
                .with_children(|parent| {
                    // Title
                    parent
                        .spawn_bundle(TextBundle::from_section(
                            "Scrolling list",
                            TextStyle {
                                font: asset_server.load("fonts/FiraSans-Bold.ttf"),
                                font_size: 25.,
                                color: Color::WHITE,
                            },
                        ))
                        .insert(SizeConstraints::suggested(Val::Undefined, Val::Px(25.)))
                        .insert(Spacing::AUTO_MARGIN);
                    // List with hidden overflow
                    parent
                        .spawn_bundle(NodeBundle {
                            size_constraints: SizeConstraints::suggested(
                                Val::Percent(100.0),
                                Val::Percent(50.0),
                            ),
                            overflow: Overflow::Hidden,
                            flex_layout: FlexLayout {
                                flex_direction: FlexDirection::ColumnReverse,
                                align_self: AlignSelf::Center,
                                ..default()
                            },
                            color: Color::rgb(0.10, 0.10, 0.10).into(),
                            ..default()
                        })
                        .with_children(|parent| {
                            // Moving panel
                            parent
                                .spawn_bundle(NodeBundle {
                                    size_constraints: SizeConstraints::max(
                                        Val::Undefined,
                                        Val::Undefined,
                                    ),
                                    flex_layout: FlexLayout {
                                        flex_direction: FlexDirection::ColumnReverse,
                                        grow: 1.0,
                                        ..default()
                                    },
                                    color: Color::NONE.into(),
                                    ..default()
                                })
                                .insert(ScrollingList::default())
                                .with_children(|parent| {
                                    // List items
                                    for i in 0..30 {
                                        parent
                                            .spawn_bundle(
                                                TextBundle::from_section(
                                                    format!("Item {i}"),
                                                    TextStyle {
                                                        font: asset_server
                                                            .load("fonts/FiraSans-Bold.ttf"),
                                                        font_size: 20.,
                                                        color: Color::WHITE,
                                                    },
                                                )
                                                .with_layout(FlexLayout {
                                                    shrink: 0.,
                                                    ..default()
                                                }),
                                            )
                                            .insert(Spacing::AUTO_MARGIN)
                                            .insert(SizeConstraints::suggested(
                                                Val::Undefined,
                                                Val::Px(20.),
                                            ));
                                    }
                                });
                        });
                });
            // absolute positioning
            parent
                .spawn_bundle(NodeBundle {
                    size_constraints: SizeConstraints::suggested(Val::Px(200.0), Val::Px(200.0)),
                    position_type: PositionType::Absolute,
                    offset: Offset(UiRect {
                        left: Val::Px(210.0),
                        bottom: Val::Px(10.0),
                        ..default()
                    }),
                    spacing: Spacing::border(UiRect::all(Val::Px(20.0))),
                    color: Color::rgb(0.4, 0.4, 1.0).into(),
                    ..default()
                })
                .with_children(|parent| {
                    parent.spawn_bundle(NodeBundle {
                        size_constraints: SizeConstraints::FULL,
                        color: Color::rgb(0.8, 0.8, 1.0).into(),
                        ..default()
                    });
                });
            // render order test: reddest in the back, whitest in the front (flex center)
            parent
                .spawn_bundle(NodeBundle {
                    size_constraints: SizeConstraints::FULL,
                    position_type: PositionType::Absolute,
                    flex_layout: FlexLayout {
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    color: Color::NONE.into(),
                    ..default()
                })
                .with_children(|parent| {
                    parent
                        .spawn_bundle(NodeBundle {
                            size_constraints: SizeConstraints::FULL,
                            color: Color::rgb(1.0, 0.0, 0.0).into(),
                            ..default()
                        })
                        .with_children(|parent| {
                            parent.spawn_bundle(NodeBundle {
                                size_constraints: SizeConstraints::FULL,
                                position_type: PositionType::Absolute,
                                offset: Offset(UiRect {
                                    left: Val::Px(20.0),
                                    bottom: Val::Px(20.0),
                                    ..default()
                                }),
                                color: Color::rgb(1.0, 0.3, 0.3).into(),
                                ..default()
                            });
                            parent.spawn_bundle(NodeBundle {
                                size_constraints: SizeConstraints::FULL,
                                position_type: PositionType::Absolute,
                                offset: Offset(UiRect {
                                    left: Val::Px(40.0),
                                    bottom: Val::Px(40.0),
                                    ..default()
                                }),
                                color: Color::rgb(1.0, 0.5, 0.5).into(),
                                ..default()
                            });
                            parent.spawn_bundle(NodeBundle {
                                size_constraints: SizeConstraints::FULL,
                                position_type: PositionType::Absolute,
                                offset: Offset(UiRect {
                                    left: Val::Px(60.0),
                                    bottom: Val::Px(60.0),
                                    ..default()
                                }),
                                color: Color::rgb(1.0, 0.7, 0.7).into(),
                                ..default()
                            });
                            // alpha test
                            parent.spawn_bundle(NodeBundle {
                                size_constraints: SizeConstraints::suggested(
                                    Val::Px(100.0),
                                    Val::Px(100.0),
                                ),
                                position_type: PositionType::Absolute,
                                offset: Offset(UiRect {
                                    left: Val::Px(80.0),
                                    bottom: Val::Px(80.0),
                                    ..default()
                                }),
                                color: Color::rgba(1.0, 0.9, 0.9, 0.4).into(),
                                ..default()
                            });
                        });
                });
            // bevy logo (flex center)
            parent
                .spawn_bundle(NodeBundle {
                    size_constraints: SizeConstraints::FULL,
                    position_type: PositionType::Absolute,
                    flex_layout: FlexLayout {
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::FlexEnd,
                        ..default()
                    },
                    color: Color::NONE.into(),
                    ..default()
                })
                .with_children(|parent| {
                    // bevy logo (image)
                    parent.spawn_bundle(ImageBundle {
                        size_constraints: SizeConstraints::suggested(Val::Px(500.0), Val::Auto),
                        image: asset_server.load("branding/bevy_logo_dark_big.png").into(),
                        ..default()
                    });
                });
        });
}

#[derive(Component, Default)]
struct ScrollingList {
    position: f32,
}

fn mouse_scroll(
    mut mouse_wheel_events: EventReader<MouseWheel>,
    mut query_list: Query<(&mut ScrollingList, &mut Offset, &Children, &Node)>,
    query_item: Query<&Node>,
) {
    for mouse_wheel_event in mouse_wheel_events.iter() {
        for (mut scrolling_list, mut offset, children, uinode) in &mut query_list {
            let items_height: f32 = children
                .iter()
                .map(|entity| query_item.get(*entity).unwrap().size.y)
                .sum();
            let panel_height = uinode.size.y;
            let max_scroll = (items_height - panel_height).max(0.);
            let dy = match mouse_wheel_event.unit {
                MouseScrollUnit::Line => mouse_wheel_event.y * 20.,
                MouseScrollUnit::Pixel => mouse_wheel_event.y,
            };
            scrolling_list.position += dy;
            scrolling_list.position = scrolling_list.position.clamp(-max_scroll, 0.);
            offset.top = Val::Px(scrolling_list.position);
        }
    }
}
