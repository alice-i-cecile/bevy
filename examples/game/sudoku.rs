use bevy::prelude::*;
use setup::GRID_LEFT_EDGE;

fn main() {
    App::build()
        // Changes the background color to white
        .insert_resource(ClearColor(Color::rgb(1.0, 1.0, 1.0)))
        .add_plugins(DefaultPlugins)
        .add_plugin(setup::SetupPlugin)
        .add_plugin(sudoku_rules::SudokuRulesPlugin)
        .add_plugin(interaction::InteractionPlugin)
        .run();
}

mod setup {
    use bevy::prelude::*;

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
            transform: Transform::from_xyz(x, y, 0.0),
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

    pub struct Cell;
    pub struct Coordinates {
        pub row: u8,
        pub column: u8,
        /// Squares are counted from 1 to 9 starting at the top left,
        /// in standard left-to-right reading order
        ///
        /// The standard term for the 3x3 box a cell is in is `box`,
        /// but that's a reserved word in Rust
        pub square: u8,
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
                    transform: Transform::from_xyz(x, y, -1.0),
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

mod sudoku_rules {
    use bevy::prelude::*;
    use sudoku::Sudoku;
    pub struct SudokuRulesPlugin;

    impl Plugin for SudokuRulesPlugin {
        fn build(&self, app: &mut AppBuilder) {
            app;
        }
    }
}

mod interaction {
    use bevy::prelude::*;
    pub struct InteractionPlugin;

    impl Plugin for InteractionPlugin {
        fn build(&self, app: &mut AppBuilder) {
            app;
        }
    }
}
