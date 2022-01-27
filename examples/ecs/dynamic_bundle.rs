//! Dynamic collections of components can be very expressive,
//! but the [`Bundle`] trait isn't object safe!
//! To work around this, we can use the [`ApplicableBundle`] subtrait instead.

use bevy::prelude::*;
use confetti_button::ConfettiButton;
use moving_button::MovingButton;

// In this example, we're showcasing the expressivity of dynamic bundles
// by abstracting over a type of entity using the `Widget` trait
//
// Each memeber of this trait shares some common functionality,
// but has a great deal of flexibility.
// We could use an enum rather that a trait, but that would prevent us from using trait methods,
// and would be much harder to extend with new members.
fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_startup_system(spawn_camera)
        // Trait-constrained generic systems are extremely useful in combination with dynamic bundles,
        // allowing you to reuse logic without losing access to the trait methods you care about
        // This pattern can be extended using trait extension methods,
        // allowing you to add multiple systems and resources to the `App` in a single method call.
        .add_widget::<ConfettiButton>()
        // Obviously, this level of abstraction is wildly inapproriate for n=2
        // and should be reserved for cases where you truly need bundles whose identity cannot be known at compile time
        // or need to operate over a massive number of closely related types
        .add_widget::<MovingButton>()
        .add_system(apply_velocity)
        .add_system(bounce_off_walls)
        .run();
}

/// An interactable UI element with custom behavior
///
// The `Component` trait bound allows us to automically insert a marker component on each entity
// The methods here must be implemented by every type that has this trait,
// establishing a common minimal set of properties and behavior
trait Widget: Component {
    /// Creates a bundle which can be used to spawn a new widget
    fn new(&self) -> Box<dyn ApplicableBundle>;

    /// How many widgets should we spawn at startup?
    fn n_to_spawn() -> u8;

    /// Does arbitrarily exciting things when pressed!
    fn on_press(&self, commands: &mut Commands);

    /// Widgets change color when hovered, based on their internal state
    ///
    /// This pattern is much less exciting than the `Commands` pattern above,
    /// but it's immediate and dramatically easier to reason about.
    /// Try to scope the behavior whenever possible.
    fn on_hover(&self, query: Query<(&Self)>) -> UiColor;
}

// This is a trait extension method,
// and is only ever intended to be implemented for `App`
trait WidgetExtension {
    fn add_widget<W: Widget>(&mut self) {}
}

impl WidgetExtension for App {
    // Here, we can quickly add multiple systems, resource and so on
    fn add_widget<W: Widget>(&mut self) {
        // Each of these systems will have affect a specific kind of widget,
        // running in parallel, and relies on the methods on Widget to customize behavior
        app.add_system(spawn_widget::<W>)
            .add_system(press_widget::<W>)
            .add_system(hover_widget::<W>);
    }
}

fn spawn_widget<W: Widget>(mut commands: Commands) {
    // `W::n_to_spawn` calls the trait method on `Widget` for the concrete type `W`
    for i in 0..W::n_to_spawn() {
        commands
            .spawn()
            // A marker component, useful for filtering in systems
            .insert(W)
            // All of our widgets must be interactable,
            // so we insert the `Interaction` component automatically
            .insert(Interaction::None)
            // All of our widgets need a color too
            // By inserting these components first,
            // the can be overwritten by the dynamic bundle later (for better or worse)
            .insert(UiColor)
            // The dynamic bit!
            .insert_bundle(W::new());
    }
}

// The `With<W>` filter is essential here, otherwise we'd apply this behavior to *all* interactable entitites
fn press_widget<W: Widget>(query: Query<&Interaction, With<W>>, mut commands: Commands) {
    for interaction in query.iter() {
        if interaction == Interaction::Pressed {
            // Who *knows* what sort of commands this widget might issue!
            W::on_press(&mut commands);
        }
    }
}

// By scoping the behavior in our trait, we can write much more constrained systems
fn hover_widget<W: Widget>(mut query: Query<(&Interaction, &mut UiColor), With<W>>) {
    for (interaction, mut color) in query.iter() {
        if interaction == Interaction::Hovered {
            // Change the button's color when hovered
            *color = W::on_hover(&mut commands);
        }
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn_bundle(UiCameraBundle::default());
}

mod confetti_button {
    struct ConfettiButton {
        confetti_color: ConfettiColor,
    }

    struct ConfettiColor {
        color: Color,
    }

    impl Widget for ConfettiButton {
        fn new(&self) -> Box<dyn ApplicableBundle> {
            Box::new()
        }
    }
}

mod moving_button {}
