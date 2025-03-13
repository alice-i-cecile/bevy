use crate::{component::Tick, entity::Entity, error::BevyError, resource::Resource};
use alloc::borrow::Cow;

/// Additional context for an ECS operation that failed.
pub enum EcsErrorContext {
    /// A system failed.
    System {
        /// The name of the system that failed.
        name: Cow<'static, str>,
        /// The last tick that the system was run.
        last_run: Tick,
    },
    /// An observer failed.
    Observer {
        /// The name of the observer that failed.
        name: Cow<'static, str>,
        /// The last tick that the observer was run.
        last_run: Tick,
    },
    /// A command failed.
    Command {
        /// The name of the command that failed.
        name: Cow<'static, str>,
    },
    /// An entity command failed.
    EntityCommand {
        /// The name of the entity command that failed.
        name: Cow<'static, str>,
        /// The entity that the command was run on.
        entity: Entity,
    },
}

impl EcsErrorContext {
    /// A string describing the kind of ECS operation that failed.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::System { .. } => "system",
            Self::Observer { .. } => "observer",
            Self::Command { .. } => "command",
            Self::EntityCommand { .. } => "entity command",
        }
    }

    /// The name of the ECS operation that failed.
    pub fn name(&self) -> &Cow<'static, str> {
        match self {
            Self::System { name, .. }
            | Self::Observer { name, .. }
            | Self::Command { name, .. }
            | Self::EntityCommand { name, .. } => name,
        }
    }
}

/// The error handler of last resort used for [`bevy_ecs::error::Result`]s returned by systems, commands and observers,
/// when an error is not otherwise handled.
///
/// This is stored as a resource in the [`World`](crate::world::World),
/// and defaults to panicking if not set.
///
/// See [`bevy_ecs::error`] for more information on error handling,
/// and [`bevy_ecs::error::handler`] for an assortment of built-in error handlers.
pub struct FallbackErrorHandler(pub fn(BevyError, EcsErrorContext));

impl Resource for FallbackErrorHandler {}

impl Default for FallbackErrorHandler {
    fn default() -> Self {
        Self(panic)
    }
}

macro_rules! inner {
    ($call:path, $e:ident, $c:ident) => {
        $call!(
            "Encountered an error in {} `{}`: {:?}",
            $c.kind(),
            $c.name(),
            $e
        );
    };
}

/// Error handler that panics with the system error.
#[track_caller]
#[inline]
pub fn panic(error: BevyError, ctx: EcsErrorContext) {
    inner!(panic, error, ctx);
}

/// Error handler that logs the system error at the `error` level.
#[track_caller]
#[inline]
pub fn error(error: BevyError, ctx: EcsErrorContext) {
    inner!(log::error, error, ctx);
}

/// Error handler that logs the system error at the `warn` level.
#[track_caller]
#[inline]
pub fn warn(error: BevyError, ctx: EcsErrorContext) {
    inner!(log::warn, error, ctx);
}

/// Error handler that logs the system error at the `info` level.
#[track_caller]
#[inline]
pub fn info(error: BevyError, ctx: EcsErrorContext) {
    inner!(log::info, error, ctx);
}

/// Error handler that logs the system error at the `debug` level.
#[track_caller]
#[inline]
pub fn debug(error: BevyError, ctx: EcsErrorContext) {
    inner!(log::debug, error, ctx);
}

/// Error handler that logs the system error at the `trace` level.
#[track_caller]
#[inline]
pub fn trace(error: BevyError, ctx: EcsErrorContext) {
    inner!(log::trace, error, ctx);
}

/// Error handler that ignores the system error.
#[track_caller]
#[inline]
pub fn ignore(_: BevyError, _: EcsErrorContext) {}
