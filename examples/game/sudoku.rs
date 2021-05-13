use bevy::prelude::*;

fn main() {
    App::build()
        .add_plugins(DefaultPlugins)
        .add_plugin(setup::SetupPlugin)
        .add_plugin(sudoku_rules::SudokuRulesPlugin)
        .add_plugin(interaction::InteractionPlugin)
        .run();
}

mod setup {
    use bevy::prelude::*;

    pub const CELL_SIZE: f32 = 50.0;
    pub const GRID_SIZE: f32 = 9.0 * CELL_SIZE;
    pub const MINOR_LINE_THICKNESS: f32 = 5.0;
    pub const MAJOR_LINE_THICKNESS: f32 = 10.0;
    // Defines the bottom left corner of the grid in absolute coordinates
    // (0, 0) is in the center of the screen in Bevy
    pub const GRID_LEFT_EDGE: f32 = -100.0;
    pub const GRID_BOTTOM_EDGE: f32 = -100.0;

    pub struct SetupPlugin;

    impl Plugin for SetupPlugin {
        fn build(&self, app: &mut AppBuilder) {
            app.add_system(spawn_camera.system())
                .add_system(spawn_grid.system());
        }
    }

    fn spawn_camera(mut commands: Commands) {
        commands.spawn_bundle(OrthographicCameraBundle::new_2d());
    }

    fn spawn_grid(mut commands: Commands) {
        for row in 0..=9 {
            commands.spawn_bundle(new_gridline(Orientation::Horizontal, row));
        }

        for column in 0..=9 {
            commands.spawn_bundle(new_gridline(Orientation::Vertical, column));
        }
    }

    enum Orientation {
        Horizontal,
        Vertical,
    }

    fn new_gridline(orientation: Orientation, i: u8) -> SpriteBundle {
        // The grid lines that define the boxes need to be thicker
        let thickness = if (i % 3) == 0 {
            MAJOR_LINE_THICKNESS
        } else {
            MINOR_LINE_THICKNESS
        };

        let size = match orientation {
            Orientation::Horizontal => Vec2::new(GRID_SIZE + thickness, thickness),
            Orientation::Vertical => Vec2::new(thickness, GRID_SIZE + thickness),
        };

        let offset = i as f32 * CELL_SIZE - 0.5 * GRID_SIZE;

        let (x, y) = match orientation {
            Orientation::Horizontal => (GRID_LEFT_EDGE, GRID_BOTTOM_EDGE + offset),
            Orientation::Vertical => (GRID_LEFT_EDGE + offset, GRID_BOTTOM_EDGE),
        };

        SpriteBundle {
            sprite: Sprite::new(size),
            transform: Transform::from_xyz(x, y, 0.0),
            ..Default::default()
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
