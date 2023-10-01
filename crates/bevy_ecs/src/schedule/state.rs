use std::fmt::Debug;
use std::hash::Hash;
use std::mem;
use std::ops::Deref;

use crate as bevy_ecs;
use crate::change_detection::DetectChangesMut;
#[cfg(feature = "bevy_reflect")]
use crate::reflect::ReflectResource;
use crate::schedule::ScheduleLabel;
use crate::system::Resource;
use crate::world::World;
#[cfg(feature = "bevy_reflect")]
use bevy_reflect::std_traits::ReflectDefault;

pub use bevy_ecs_macros::States;

/// Types that can define world-wide states in a finite-state machine.
///
/// The [`Default`] trait defines the starting state.
/// Multiple states can be defined for the same world,
/// allowing you to classify the state of the world across orthogonal dimensions.
/// You can access the current state of type `T` with the [`State<T>`] resource,
/// and the queued state with the [`NextState<T>`] resource.
///
/// State transitions typically occur in the [`OnEnter<T::Variant>`] and [`OnExit<T:Variant>`] schedules,
/// which can be run via the [`apply_state_transition::<T>`] system.
///
/// # Examples
///
/// States are commonly defined as enums, with the [`States`] derive macro.
///
/// ```rust
/// use bevy_ecs::prelude::States;
///
/// #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, States)]
/// enum GameState {
///  #[default]
///   MainMenu,
///   SettingsMenu,
///   InGame,
/// }
///
/// // You can have multiple states for the same world / app:
/// // each state is independent of the others and stored in its own resources.
/// #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, States)]
/// enum GameMode {
///     #[default]
///     SinglePlayer,
///     Tutorial,
///     MultiPlayer,
/// }
/// ```
///
/// But states aren't limited to simple dataless enums:
///
/// ```rust
/// use bevy_ecs::prelude::States;
///
/// #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, States)]
/// struct Level(u32);
/// ```
///
/// You can even nest enums inside of other enums, creating a "sub-state" pattern.
/// This can be useful for complex state machines to ensure that invalid states are unrepresentable.
///
/// ```rust {
/// use bevy_ecs::prelude::States;
///
/// #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, States)]
/// enum AppState {
///     #[default]
///     Loading,
///     MainMenu,
///     Playing {
///        paused: bool,
///        game_mode: GameMode,
///     }
/// }
///
/// // Note that we're *not* deriving `States` for `GameMode` here:
/// // we don't want to be able to set the game mode without also setting the `AppState::Playing` state.
/// #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
/// enum GameMode {
///     #[default]
///     SinglePlayer,
///     Tutorial,
///     MultiPlayer,
/// }
/// ```
pub trait States: 'static + Send + Sync + Clone + PartialEq + Eq + Hash + Debug + Default {}

/// A state or set of states that can be matched against.
///
/// This is used to determine if a schedule such as [`OnEnter`], [`OnExit`] or [`OnTransition`] should run when a state is entered or exited.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MatchedState<S: States> {
    /// Only the exact state is matched.
    Exact(S),
    /// Any state that matches the given predicate is matched.
    Pattern(StatePredicate<S>),
}

#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub struct StatePredicate<S: States> {
    func: Box<dyn Fn(&S) -> bool>,
}

impl<S: States> MatchedState<S> {
    /// Returns `true` if the given state matches this state.
    pub fn matches(&self, state: &S) -> bool {
        match self {
            MatchedState::Exact(s) => s == state,
            MatchedState::Pattern(predicate) => (predicate.func)(state),
        }
    }
}

/// The label of a [`Schedule`](super::Schedule) that runs whenever [`State<S>`]
/// enters this state.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct OnEnter<S: States>(pub MatchedState<S>);

impl<S: States> OnEnter<S> {
    /// A schedule that runs whenever [`State<S>`] enters a new state that is equal to `state`.
    pub fn exact(state: S) -> Self {
        OnEnter(MatchedState::Exact(state))
    }

    /// A schedule that runs whenever [`State<S>`] enters a new state that matches `predicate`.
    pub fn pattern(predicate: impl Fn(&S) -> bool + 'static) -> Self {
        OnEnter(MatchedState::Pattern(StatePredicate {
            func: Box::new(predicate),
        }))
    }

    /// Returns `true` if the new state matches this state.
    pub fn matches(&self, new_state: &S) -> bool {
        self.0.matches(new_state)
    }
}

/// The label of a [`Schedule`](super::Schedule) that runs whenever [`State<S>`]
/// exits this state.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct OnExit<S: States>(pub MatchedState<S>);

impl<S: States> OnExit<S> {
    /// A schedule that runs whenever [`State<S>`] leaves a state that is equal to `state`.
    pub fn exact(state: S) -> Self {
        OnExit(MatchedState::Exact(state))
    }

    /// A schedule that runs whenever [`State<S>`] leaves a new state that matches `predicate`.
    pub fn pattern(predicate: impl Fn(&S) -> bool + 'static) -> Self {
        OnExit(MatchedState::Pattern(StatePredicate {
            func: Box::new(predicate),
        }))
    }

    /// Returns `true` if the previous state matches this state.
    pub fn matches(&self, old_state: &S) -> bool {
        self.0.matches(old_state)
    }
}

/// The label of a [`Schedule`](super::Schedule) that **only** runs whenever [`State<S>`]
/// exits the `from` state, AND enters the `to` state.
///
/// Systems added to this schedule are always ran *after* [`OnExit`], and *before* [`OnEnter`].
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct OnTransition<S: States> {
    /// The state being exited.
    pub from: MatchedState<S>,
    /// The state being entered.
    pub to: MatchedState<S>,
}

impl<S: States> OnTransition<S> {
    /// A schedule that runs whenever [`State<S>`] leaves a state that is equal to `state`.
    pub fn exact(old_state: S, new_state: S) -> Self {
        OnTransition {
            from: MatchedState::Exact(old_state),
            to: MatchedState::Exact(new_state),
        }
    }

    /// A schedule that runs whenever [`State<S>`] leaves a new state that matches `predicate`.
    pub fn pattern(
        from_predicate: impl Fn(&S) -> bool + 'static,
        to_predicate: impl Fn(&S) -> bool + 'static,
    ) -> Self {
        OnTransition {
            from: MatchedState::Pattern(StatePredicate {
                func: Box::new(from_predicate),
            }),
            to: MatchedState::Pattern(StatePredicate {
                func: Box::new(to_predicate),
            }),
        }
    }

    /// Returns `true` if the previous state matches this state.
    pub fn matches(&self, old_state: &S, new_state: &S) -> bool {
        self.from.matches(old_state) && self.to.matches(new_state)
    }
}

/// A finite-state machine whose transitions have associated schedules
/// ([`OnEnter(state)`] and [`OnExit(state)`]).
///
/// The current state value can be accessed through this resource. To *change* the state,
/// queue a transition in the [`NextState<S>`] resource, and it will be applied by the next
/// [`apply_state_transition::<S>`] system.
///
/// The starting state is defined via the [`Default`] implementation for `S`.
#[derive(Resource, Default, Debug)]
#[cfg_attr(
    feature = "bevy_reflect",
    derive(bevy_reflect::Reflect),
    reflect(Resource, Default)
)]
pub struct State<S: States>(S);

impl<S: States> State<S> {
    /// Creates a new state with a specific value.
    ///
    /// To change the state use [`NextState<S>`] rather than using this to modify the `State<S>`.
    pub fn new(state: S) -> Self {
        Self(state)
    }

    /// Get the current state.
    pub fn get(&self) -> &S {
        &self.0
    }
}

impl<S: States> PartialEq<S> for State<S> {
    fn eq(&self, other: &S) -> bool {
        self.get() == other
    }
}

impl<S: States> Deref for State<S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

/// The next state of [`State<S>`].
///
/// To queue a transition, just set the contained value to `Some(next_state)`.
/// Note that these transitions can be overridden by other systems:
/// only the actual value of this resource at the time of [`apply_state_transition`] matters.
#[derive(Resource, Default, Debug)]
#[cfg_attr(
    feature = "bevy_reflect",
    derive(bevy_reflect::Reflect),
    reflect(Resource, Default)
)]
pub struct NextState<S: States>(pub Option<S>);

impl<S: States> NextState<S> {
    /// Tentatively set a planned state transition to `Some(state)`.
    pub fn set(&mut self, state: S) {
        self.0 = Some(state);
    }
}

/// Run the enter schedule (if it exists) for the current state.
pub fn run_enter_schedule<S: States>(world: &mut World) {
    world
        .try_run_schedule(OnEnter(world.resource::<State<S>>().0.clone()))
        .ok();
}

/// If a new state is queued in [`NextState<S>`], this system:
/// - Takes the new state value from [`NextState<S>`] and updates [`State<S>`].
/// - Runs the [`OnExit(exited_state)`] schedule, if it exists.
/// - Runs the [`OnTransition { from: exited_state, to: entered_state }`](OnTransition), if it exists.
/// - Runs the [`OnEnter(entered_state)`] schedule, if it exists.
///
/// These schedules are run in the order listed above: [`OnExit`] is always run first, then [`OnTransition`], then [`OnEnter`].
pub fn apply_state_transition<S: States>(world: &mut World) {
    // We want to take the `NextState` resource,
    // but only mark it as changed if it wasn't empty.
    let mut next_state_resource = world.resource_mut::<NextState<S>>();
    if let Some(entered) = next_state_resource.bypass_change_detection().0.take() {
        next_state_resource.set_changed();

        let mut state_resource = world.resource_mut::<State<S>>();
        if *state_resource != entered {
            let exited = mem::replace(&mut state_resource.0, entered.clone());
            // Try to run the schedules if they exist.
            world.try_run_schedule(OnExit(exited.clone())).ok();
            world
                .try_run_schedule(OnTransition {
                    from: exited,
                    to: entered.clone(),
                })
                .ok();
            world.try_run_schedule(OnEnter(entered)).ok();
        }
    }
}
