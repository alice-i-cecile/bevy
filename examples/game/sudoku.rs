use bevy::prelude::*;

fn main() {
    App::build()
        .insert_resource(ClearColor(interaction::BACKGROUND_COLOR))
        .add_plugins(DefaultPlugins)
        .add_plugin(setup::SetupPlugin)
        .add_plugin(interaction::InteractionPlugin)
        .run();
}

struct Cell;
struct Coordinates {
    pub row: u8,
    pub column: u8,
    /// Squares are counted from 1 to 9 starting at the top left,
    /// in standard left-to-right reading order
    ///
    /// The standard term for the 3x3 box a cell is in is `box`,
    /// but that's a reserved word in Rust
    pub square: u8,
}

mod setup {
    use super::*;

    pub const CELL_SIZE: f32 = 30.0;
    pub const GRID_SIZE: f32 = 9.0 * CELL_SIZE;
    pub const MINOR_LINE_THICKNESS: f32 = 2.0;
    pub const MAJOR_LINE_THICKNESS: f32 = 4.0;
    // Defines the center lines of the grid in absolute coordinates
    // (0, 0) is in the center of the screen in Bevy
    pub const GRID_CENTER_X: f32 = 0.0;
    pub const GRID_LEFT_EDGE: f32 = GRID_CENTER_X - 0.5 * GRID_SIZE;
    pub const GRID_CENTER_Y: f32 = 0.0;
    pub const GRID_BOT_EDGE: f32 = GRID_CENTER_Y - 0.5 * GRID_SIZE;
    pub const GRID_COLOR: Color = Color::rgb(0.1, 0.1, 0.1);

    pub struct SetupPlugin;

    impl Plugin for SetupPlugin {
        fn build(&self, app: &mut AppBuilder) {
            app.add_startup_system(spawn_camera.system())
                .add_startup_system(spawn_grid.system())
                .add_startup_system(spawn_cells.system());
        }
    }

    fn spawn_camera(mut commands: Commands) {
        commands.spawn_bundle(OrthographicCameraBundle::new_2d());
    }

    fn spawn_grid(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
        let grid_handle = materials.add(GRID_COLOR.into());

        for row in 0..=9 {
            commands.spawn_bundle(new_gridline(
                Orientation::Horizontal,
                row,
                grid_handle.clone(),
            ));
        }

        for column in 0..=9 {
            commands.spawn_bundle(new_gridline(
                Orientation::Vertical,
                column,
                grid_handle.clone(),
            ));
        }
    }

    enum Orientation {
        Horizontal,
        Vertical,
    }

    fn new_gridline(
        orientation: Orientation,
        i: u8,
        grid_handle: Handle<ColorMaterial>,
    ) -> SpriteBundle {
        // The grid lines that define the boxes need to be thicker
        let thickness = if (i % 3) == 0 {
            MAJOR_LINE_THICKNESS
        } else {
            MINOR_LINE_THICKNESS
        };

        let length = GRID_SIZE + thickness;

        let size = match orientation {
            Orientation::Horizontal => Vec2::new(length, thickness),
            Orientation::Vertical => Vec2::new(thickness, length),
        };

        // Each objects' position is defined by its center
        let offset = i as f32 * CELL_SIZE;

        let (x, y) = match orientation {
            Orientation::Horizontal => (GRID_LEFT_EDGE + 0.5 * GRID_SIZE, GRID_BOT_EDGE + offset),
            Orientation::Vertical => (GRID_LEFT_EDGE + offset, GRID_BOT_EDGE + 0.5 * GRID_SIZE),
        };

        SpriteBundle {
            sprite: Sprite::new(size),
            // We want these grid lines to cover any cell that it might overlap with
            transform: Transform::from_xyz(x, y, 1.0),
            material: grid_handle,
            ..Default::default()
        }
    }

    fn spawn_cells(mut commands: Commands) {
        for row in 1..=9 {
            for column in 1..=9 {
                commands.spawn_bundle(CellBundle::new(row, column));
            }
        }
    }

    #[derive(Bundle)]
    struct CellBundle {
        cell: Cell,
        coordinates: Coordinates,
        value: Option<u8>,
        #[bundle]
        cell_fill: SpriteBundle,
    }

    impl CellBundle {
        fn new(row: u8, column: u8) -> Self {
            let x = GRID_LEFT_EDGE + CELL_SIZE * row as f32 - 0.5 * CELL_SIZE;
            let y = GRID_BOT_EDGE + CELL_SIZE * column as f32 - 0.5 * CELL_SIZE;

            CellBundle {
                cell: Cell,
                coordinates: Coordinates {
                    row,
                    column,
                    square: Self::compute_square(row, column),
                },
                // No digits are filled in to begin with
                value: None,
                cell_fill: SpriteBundle {
                    // The material for this sprite begins with the same material as our background
                    sprite: Sprite::new(Vec2::new(CELL_SIZE, CELL_SIZE)),
                    // We want this cell to be covered by any grid lines that it might overlap with
                    transform: Transform::from_xyz(x, y, 0.0),
                    ..Default::default()
                },
            }
        }

        /// Computes which box a cell is in based on its row and column
        fn compute_square(row: u8, column: u8) -> u8 {
            let possible_squares_r = match row % 3 {
                0 => [1, 2, 3],
                1 => [4, 5, 6],
                2 => [7, 8, 9],
                _ => unreachable!("Remainder when divided by 3 must alway be between 0 and 2"),
            };
            let possible_squares_c = match column % 3 {
                0 => [1, 4, 7],
                1 => [2, 5, 8],
                2 => [3, 6, 9],
                _ => unreachable!("Remainder when divided by 3 must alway be between 0 and 2"),
            };

            // The square that our cell is in is given by
            // the intersection of possible squares cells with its row could be in
            // with the possible squares cells with its column could be in
            for i in possible_squares_r.iter() {
                for j in possible_squares_c.iter() {
                    if *i == *j {
                        return *i;
                    }
                }
            }
            unreachable!("Each set in possible_squares_r shares exactly one element with each set in possible_squares_c");
        }
    }
}

mod interaction {
    use bevy::{render::camera::Camera, utils::HashMap};

    use super::*;
    pub struct InteractionPlugin;

    // Marker component for selected cells
    #[derive(Debug)]
    pub struct Selected;

    // Various colors for our cells
    struct BackgroundColor(Handle<ColorMaterial>);
    pub const BACKGROUND_COLOR: Color = Color::rgb(1.0, 1.0, 1.0);
    struct SelectionColor(Handle<ColorMaterial>);
    pub const SELECTION_COLOR: Color = Color::rgb(0.8, 0.8, 0.8);

    impl Plugin for InteractionPlugin {
        fn build(&self, app: &mut AppBuilder) {
            app.add_startup_system(cell_colors.system())
                .init_resource::<CellIndex>()
                // Should run before input to ensure mapping from position to cell is correct
                .add_system(index_cells.system().before("input"))
                .add_system(mouse_selection.system().label("input"))
                // Should run after input to avoid delays
                .add_system(color_selected.system().after("input"));
        }
    }

    fn cell_colors(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
        commands.insert_resource(BackgroundColor(materials.add(BACKGROUND_COLOR.into())));
        commands.insert_resource(SelectionColor(materials.add(SELECTION_COLOR.into())));
    }

    fn mouse_selection(
        cell_query: Query<(Entity, Option<&Selected>), With<Cell>>,
        camera_query: Query<&Transform, With<Camera>>,
        mouse_button_input: Res<Input<MouseButton>>,
        windows: Res<Windows>,
        cell_index: Res<CellIndex>,
        mut commands: Commands,
    ) {
        if mouse_button_input.just_pressed(MouseButton::Left) {
            // Our game only has one window
            let window = windows.get_primary().unwrap();
            // These coordinates are in terms of the window's coordinates
            // and must be converted to the world coordinates used by our cell
            let mut cursor_position = window.cursor_position().unwrap();
            // FIXME: use https://github.com/bevyengine/bevy/pull/1799 once merged instead
            let camera_transform = camera_query.single().unwrap();
            let window_size = Vec2::new(window.width() as f32, window.height() as f32);

            // World coordinates are measured from the center
            // while screen coordinates are measures from the bottom left.
            cursor_position -= 0.5 * window_size;

            // Apply the camera's transform to correct for scale, angle etc.
            // Returning a quaternion
            let world_quat =
                camera_transform.compute_matrix() * cursor_position.extend(0.0).extend(1.0);

            let cursor_position_world = Vec2::new(world_quat.x, world_quat.y);

            // Use the CellIndex resource to map the mouse position to a particular cell
            let selected_cell = cell_index.get(cursor_position_world);

            if let Some(entity) = selected_cell {
                let (_, maybe_selected) = cell_query.get(entity).unwrap();
                match maybe_selected {
                    // Select cells that aren't selected
                    None => commands.entity(entity).insert(Selected),
                    // Unselect cells that were already selected
                    Some(_) => commands.entity(entity).remove::<Selected>(),
                };
            } else {
                for (entity, _) in cell_query.iter() {
                    // If the user clicks outside of the grid, unselect everything
                    commands.entity(entity).remove::<Selected>();
                }
            }
        }
    }

    #[derive(Default)]
    struct CellIndex {
        pub cell_map: HashMap<Entity, BoundingBox>,
    }

    struct BoundingBox {
        pub bottom_left: Vec2,
        pub top_right: Vec2,
    }

    impl CellIndex {
        pub fn get(&self, position: Vec2) -> Option<Entity> {
            // This is a slow and naive linear-time approach to spatial indexing
            // But it works fine for 81 items!
            for (entity, bounding_box) in self.cell_map.iter() {
                // Checks if the position is in the bounding box on both x and y
                let in_bounds = position.cmpge(bounding_box.bottom_left)
                    & position.cmple(bounding_box.top_right);
                // Only returns true if it's inside the box on both x and y
                if in_bounds.all() {
                    // This early return of a single item only works correctly
                    // because we know our entitities never overlap
                    // We would need a way to break ties otherwise
                    return Some(*entity);
                }
            }
            // Return None if no matches found
            None
        }
    }

    fn index_cells(
        query: Query<(Entity, &Sprite, &Transform), (With<Cell>, Changed<Transform>)>,
        mut cell_index: ResMut<CellIndex>,
    ) {
        // Our Changed<Transform> filter ensures that this system only does work
        // on entities whose Transforms were added or mutated since the last time
        // this system ran
        for (entity, sprite, transform) in query.iter() {
            let center = transform.translation.truncate();
            let bottom_left = center - sprite.size / 2.0;
            let top_right = center + sprite.size / 2.0;

            // .insert overwrites existing values
            cell_index.cell_map.insert(
                entity,
                BoundingBox {
                    bottom_left,
                    top_right,
                },
            );
        }
    }

    fn color_selected(
        mut query: Query<(Option<&Selected>, &mut Handle<ColorMaterial>), With<Cell>>,
        background_color: Res<BackgroundColor>,
        selection_color: Res<SelectionColor>,
    ) {
        for (maybe_selected, mut material_handle) in query.iter_mut() {
            match maybe_selected {
                Some(_) => *material_handle = selection_color.0.clone(),
                None => *material_handle = background_color.0.clone(),
            }
        }
    }
}
