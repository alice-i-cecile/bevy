use crate::component::ComponentId;
use crate::schedule::{AmbiguityDetection, SystemContainer, SystemStage};
use crate::world::World;

use bevy_utils::get_short_name;
use bevy_utils::tracing::{error, warn};
use fixedbitset::FixedBitSet;
use std::hash::Hash;

/// Within a stage, writes to a component or resource (and reads relative to writes) should typically have a well-defined order.
///
/// Ambiguous order between systems that have conflicting access can result in subtle logic bugs since,
/// in the absence of constraints, systems are executed in an arbitrary order and that can change between runs
/// and even frames.
///
/// Some ambiguities reported by the ambiguity checker may be warranted (to allow two systems to run
/// without blocking each other) or spurious, as the exact combination of archetypes used may
/// prevent them from ever conflicting during actual gameplay.
/// You can resolve the warnings produced by the ambiguity checker by adding `.before` or `.after`
/// to one of the conflicting systems referencing the other system to force a specific ordering.
/// Alternatively, if you're confident the error is a false positive (and you don't need true determinism),
/// you can explicitly ignore it using the `.ambiguous_with` method.
///
/// Note that the checker reports each pairwise ambiguity:
/// typically, these can be resolved with fewer constraints than the number of ambiguities listed
/// as transitive orderings will resolve ambiguities (e.g. A before B before C will resolve an ambiguity between A and C).
/// It's generally more productive to pause and reflect on the underlying structure of your logic,
/// rather than blindly resolving one warning at a time.
///
/// By default, the value of this resource is set to `Warn`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExecutionOrderAmbiguities {
    /// Disables all checks for execution order ambiguities
    Allow,
    /// Displays only the number of unresolved ambiguities detected by the ambiguity checker
    Warn,
    /// Displays a full report of ambiguities detected by the ambiguity checker
    WarnVerbose,
    /// Verbosely reports all non-ignored ambiguities, including those between Bevy's systems
    ///
    /// These will not be actionable: you should only turn on this functionality when
    /// investigating to see if there's a Bevy bug or working on the engine itself.
    WarnInternal,
    /// Like `WarnVerbose`, but panics if any non-ignored ambiguities exist
    Deny,
    /// Verbosely reports ALL ambiguities, even ignored ones
    ///
    /// Panics if any ambiguities exist.
    ///
    /// This will be very noisy, but can be useful when attempting to track down subtle determinism issues,
    /// as you might need when attempting to implement lockstep networking.
    Forbid,
}

/// A pair of systems that have conflicting access and an ambiguous execution order.
///
/// Created by applying [`find_ambiguities`] to a [`SystemContainer`].
/// These can be reported by configuring the [`ExecutionOrderAmbiguities`] resource.
#[derive(Debug, Clone, Eq)]
pub struct SystemOrderAmbiguity {
    // The names of the conflicting systems
    pub system_names: [String; 2],
    /// The components (and resources) that these systems have incompatible access to
    pub conflicts: Vec<String>,
    /// The segment of the [`SystemStage`] that the conflicting systems were stored in
    pub segment: SystemStageSegment,
}

impl PartialEq for SystemOrderAmbiguity {
    fn eq(&self, other: &Self) -> bool {
        let mut self_names = self.system_names.clone();
        self_names.sort();

        let mut other_names = self.system_names.clone();
        other_names.sort();

        let mut self_conflicts = self.conflicts.clone();
        self_conflicts.sort();

        let mut other_conflicts = self.conflicts.clone();
        other_conflicts.sort();

        (self_names == other_names)
            && (self_conflicts == other_conflicts)
            && (self.segment == other.segment)
    }
}

// This impl is needed to allow us to test whether a returned set of ambiguities
// matches the expected value
impl Hash for SystemOrderAmbiguity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // The order of the systems doesn't matter
        let mut system_names = self.system_names.clone();
        system_names.sort();
        system_names.hash(state);
        // The order of the reported conflicts doesn't matter
        let mut conflicts = self.conflicts.clone();
        conflicts.sort();
        conflicts.hash(state);
        self.segment.hash(state);
    }
}

/// Which part of a [`SystemStage`] was a [`SystemOrderAmbiguity`] detected in?
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum SystemStageSegment {
    Parallel,
    ExclusiveAtStart,
    ExclusiveBeforeCommands,
    ExclusiveAtEnd,
}

impl SystemOrderAmbiguity {
    fn from_raw(
        system_a_index: usize,
        system_b_index: usize,
        component_ids: Vec<ComponentId>,
        segment: SystemStageSegment,
        stage: &SystemStage,
        world: &World,
    ) -> Self {
        use crate::schedule::graph_utils::GraphNode;
        use SystemStageSegment::*;

        // TODO: blocked on https://github.com/bevyengine/bevy/pull/4166
        // We can't grab the system container generically, because .parallel_systems()
        // and the exclusive equivalent return a different type,
        // and SystemContainer is not object-safe
        let (system_a_name, system_b_name) = match segment {
            Parallel => {
                let system_container = stage.parallel_systems();
                (
                    system_container[system_a_index].name(),
                    system_container[system_b_index].name(),
                )
            }
            ExclusiveAtStart => {
                let system_container = stage.exclusive_at_start_systems();
                (
                    system_container[system_a_index].name(),
                    system_container[system_b_index].name(),
                )
            }
            ExclusiveBeforeCommands => {
                let system_container = stage.exclusive_before_commands_systems();
                (
                    system_container[system_a_index].name(),
                    system_container[system_b_index].name(),
                )
            }
            ExclusiveAtEnd => {
                let system_container = stage.exclusive_at_end_systems();
                (
                    system_container[system_a_index].name(),
                    system_container[system_b_index].name(),
                )
            }
        };

        let conflicts: Vec<String> = component_ids
            .iter()
            .map(|id| world.components().get_info(*id).unwrap().name().into())
            .collect();

        Self {
            // Don't bother with Cows here
            system_names: [system_a_name.into(), system_b_name.into()],
            conflicts,
            segment,
        }
    }
}

/// Returns vector containing all pairs of indices of systems with ambiguous execution order,
/// along with specific components that have triggered the warning.
/// Systems must be topologically sorted beforehand.
pub fn find_ambiguities(
    systems: &[impl SystemContainer],
    crates_filter: &[String],
    // Should explicit attempts to ignore ambiguities be obeyed?
    report_level: ExecutionOrderAmbiguities,
) -> Vec<(usize, usize, Vec<ComponentId>)> {
    fn should_ignore_ambiguity(
        systems: &[impl SystemContainer],
        index_a: usize,
        index_b: usize,
        crates_filter: &[String],
        report_level: ExecutionOrderAmbiguities,
    ) -> bool {
        if report_level == ExecutionOrderAmbiguities::Forbid {
            return false;
        }

        let system_a = &systems[index_a];
        let system_b = &systems[index_b];

        (match system_a.ambiguity_detection() {
            AmbiguityDetection::Ignore => true,
            AmbiguityDetection::Check => false,
            AmbiguityDetection::IgnoreWithLabel(labels) => {
                labels.iter().any(|l| system_b.labels().contains(l))
            }
        }) || (match system_b.ambiguity_detection() {
            AmbiguityDetection::Ignore => true,
            AmbiguityDetection::Check => false,
            AmbiguityDetection::IgnoreWithLabel(labels) => {
                labels.iter().any(|l| system_a.labels().contains(l))
            }
        }) || (crates_filter.iter().any(|s| system_a.name().starts_with(s))
            && crates_filter.iter().any(|s| system_b.name().starts_with(s)))
    }

    let mut all_dependencies = Vec::<FixedBitSet>::with_capacity(systems.len());
    let mut all_dependants = Vec::<FixedBitSet>::with_capacity(systems.len());
    for (index, container) in systems.iter().enumerate() {
        let mut dependencies = FixedBitSet::with_capacity(systems.len());
        for &dependency in container.dependencies() {
            dependencies.union_with(&all_dependencies[dependency]);
            dependencies.insert(dependency);
            all_dependants[dependency].insert(index);
        }

        all_dependants.push(FixedBitSet::with_capacity(systems.len()));
        all_dependencies.push(dependencies);
    }
    for index in (0..systems.len()).rev() {
        let mut dependants = FixedBitSet::with_capacity(systems.len());
        for dependant in all_dependants[index].ones() {
            dependants.union_with(&all_dependants[dependant]);
            dependants.insert(dependant);
        }
        all_dependants[index] = dependants;
    }
    let mut all_relations = all_dependencies
        .drain(..)
        .zip(all_dependants.drain(..))
        .enumerate()
        .map(|(index, (dependencies, dependants))| {
            let mut relations = FixedBitSet::with_capacity(systems.len());
            relations.union_with(&dependencies);
            relations.union_with(&dependants);
            relations.insert(index);
            relations
        })
        .collect::<Vec<FixedBitSet>>();
    let mut ambiguities = Vec::new();
    let full_bitset: FixedBitSet = (0..systems.len()).collect();
    let mut processed = FixedBitSet::with_capacity(systems.len());
    for (index_a, relations) in all_relations.drain(..).enumerate() {
        for index_b in full_bitset.difference(&relations)
        {
            if !processed.contains(index_b)
                && !should_ignore_ambiguity(systems, index_a, index_b, crates_filter, report_level)
            {
                let a_access = systems[index_a].component_access();
                let b_access = systems[index_b].component_access();
                if let (Some(a), Some(b)) = (a_access, b_access) {
                    let component_ids = a.get_conflicts(b);
                    if !component_ids.is_empty() {
                        ambiguities.push((index_a, index_b, component_ids));
                    }
                } else {
                    // The ambiguity is for an exclusive system,
                    // which conflict on all data
                    ambiguities.push((index_a, index_b, Vec::default()));
                }
            }
        }
        processed.insert(index_a);
    }
    ambiguities
}

impl Default for ExecutionOrderAmbiguities {
    fn default() -> Self {
        ExecutionOrderAmbiguities::Warn
    }
}

impl SystemStage {
    /// Returns all execution order ambiguities between systems
    ///
    /// Returns 4 vectors of ambiguities for each stage, in the following order:
    /// - parallel
    /// - exclusive at start,
    /// - exclusive before commands
    /// - exclusive at end
    pub fn ambiguities(
        &mut self,
        // FIXME: these methods should not have tor rely on &mut World, or any specific World
        // see https://github.com/bevyengine/bevy/issues/4364
        world: &mut World,
        report_level: ExecutionOrderAmbiguities,
    ) -> Vec<SystemOrderAmbiguity> {
        self.initialize(world);

        if report_level == ExecutionOrderAmbiguities::Allow {
            return Vec::default();
        }

        // System order must be fresh
        debug_assert!(!self.systems_modified);

        // TODO: remove all internal ambiguities and remove this logic
        let ignored_crates = if report_level != ExecutionOrderAmbiguities::WarnInternal {
            vec![
                // Rendering
                "bevy_render".to_string(),
                "bevy_sprite".to_string(),
                "bevy_render".to_string(),
                "bevy_pbr".to_string(),
                "bevy_text".to_string(),
                "bevy_core_pipeline".to_string(),
                "bevy_ui".to_string(),
                "bevy_hierarchy".to_string(),
                // Misc
                "bevy_winit".to_string(),
                "bevy_audio".to_string(),
            ]
        } else {
            Vec::default()
        };

        let parallel: Vec<SystemOrderAmbiguity> =
            find_ambiguities(&self.parallel, &ignored_crates, report_level)
                .iter()
                .map(|(system_a_index, system_b_index, component_ids)| {
                    SystemOrderAmbiguity::from_raw(
                        *system_a_index,
                        *system_b_index,
                        component_ids.to_vec(),
                        SystemStageSegment::Parallel,
                        self,
                        world,
                    )
                })
                .collect();

        let at_start: Vec<SystemOrderAmbiguity> =
            find_ambiguities(&self.exclusive_at_start, &ignored_crates, report_level)
                .iter()
                .map(|(system_a_index, system_b_index, component_ids)| {
                    SystemOrderAmbiguity::from_raw(
                        *system_a_index,
                        *system_b_index,
                        component_ids.to_vec(),
                        SystemStageSegment::ExclusiveAtStart,
                        self,
                        world,
                    )
                })
                .collect();

        let before_commands: Vec<SystemOrderAmbiguity> = find_ambiguities(
            &self.exclusive_before_commands,
            &ignored_crates,
            report_level,
        )
        .iter()
        .map(|(system_a_index, system_b_index, component_ids)| {
            SystemOrderAmbiguity::from_raw(
                *system_a_index,
                *system_b_index,
                component_ids.to_vec(),
                SystemStageSegment::ExclusiveBeforeCommands,
                self,
                world,
            )
        })
        .collect();

        let at_end: Vec<SystemOrderAmbiguity> =
            find_ambiguities(&self.exclusive_at_end, &ignored_crates, report_level)
                .iter()
                .map(|(system_a_index, system_b_index, component_ids)| {
                    SystemOrderAmbiguity::from_raw(
                        *system_a_index,
                        *system_b_index,
                        component_ids.to_vec(),
                        SystemStageSegment::ExclusiveAtEnd,
                        self,
                        world,
                    )
                })
                .collect();

        let mut ambiguities = Vec::default();
        ambiguities.extend(at_start);
        ambiguities.extend(parallel);
        ambiguities.extend(before_commands);
        ambiguities.extend(at_end);

        ambiguities
    }

    /// Returns the number of system order ambiguities between systems in this stage
    pub fn n_ambiguities(
        &mut self,
        world: &mut World,
        report_level: ExecutionOrderAmbiguities,
    ) -> usize {
        self.ambiguities(world, report_level).len()
    }

    /// Reports all execution order ambiguities between systems
    pub fn report_ambiguities(
        &mut self,
        world: &mut World,
        report_level: ExecutionOrderAmbiguities,
    ) {
        let ambiguities = self.ambiguities(world, report_level);
        let unresolved_count = ambiguities.len();

        let mut warning_string = String::new();

        if unresolved_count > 0 {
            let plural = if unresolved_count == 1 { "" } else { "s" };
            warning_string += &format!("One of your stages contains {unresolved_count} pair{plural} of systems with unknown order and conflicting data access. \n\
				You may want to add `.before()` or `.after()` ordering constraints between some of these systems to prevent bugs.\n");

            if report_level == ExecutionOrderAmbiguities::Warn {
                warning_string += "Set the level of the `ReportExecutionOrderAmbiguities` resource to `AmbiguityReportLevel::WarnVerbose` for more details.";
            } else {
                for (i, ambiguity) in ambiguities.iter().enumerate() {
                    let ambiguity_number = i + 1;
                    // The path name is often just noise, and this gets us consistency with `conflicts`'s formatting
                    let system_a_name = get_short_name(ambiguity.system_names[0].as_str());
                    let system_b_name = get_short_name(ambiguity.system_names[1].as_str());
                    let mut conflicts: Vec<String> = ambiguity
                        .conflicts
                        .iter()
                        .map(|name| get_short_name(name.as_str()))
                        .collect();

                    // Exclusive system conflicts are reported as conflicting on the empty set
                    if conflicts.is_empty() {
                        conflicts = vec!["World".to_string()];
                    }

                    warning_string += &format!("\n{ambiguity_number:?}. `{system_a_name}` conflicts with `{system_b_name}` on {conflicts:?}");
                }
                // Print an empty line to space out multiple stages nicely
                warning_string.push('\n');
            }

            if report_level == ExecutionOrderAmbiguities::Deny
                || report_level == ExecutionOrderAmbiguities::Forbid
            {
                error!("{warning_string}");
                panic!("`ExecutionOrderAmbiguities` is set to `{report_level:?}`, which forbids the `App` from running if any two systems with incompatible data access have ambiguous execution order (unless permitted).")
            } else {
                warn!("{warning_string}");
            }
        }
    }
}

// Systems and TestResource are used in tests
#[allow(dead_code)]
#[cfg(test)]
mod tests {
    // Required to make the derive macro behave
    use crate as bevy_ecs;
    use crate::event::Events;
    use crate::prelude::*;

    struct R;

    #[derive(Component)]
    struct A;

    #[derive(Component)]
    struct B;

    // An event type
    struct E;

    fn empty_system() {}
    fn res_system(_res: Res<R>) {}
    fn resmut_system(_res: ResMut<R>) {}
    fn nonsend_system(_ns: NonSend<R>) {}
    fn nonsendmut_system(_ns: NonSendMut<R>) {}
    fn read_component_system(_query: Query<&A>) {}
    fn write_component_system(_query: Query<&mut A>) {}
    fn with_filtered_component_system(_query: Query<&mut A, With<B>>) {}
    fn without_filtered_component_system(_query: Query<&mut A, Without<B>>) {}
    fn event_reader_system(_reader: EventReader<E>) {}
    fn event_writer_system(_writer: EventWriter<E>) {}
    fn event_resource_system(_events: ResMut<Events<E>>) {}
    fn read_world_system(_world: &World) {}
    fn write_world_system(_world: &mut World) {}

    // Tests for conflict detection
    #[test]
    fn one_of_everything() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();

        test_stage
            .add_system(empty_system)
            // nonsendmut system deliberately conflicts with resmut system
            .add_system(resmut_system)
            .add_system(write_component_system)
            .add_system(event_writer_system);

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Forbid),
            0
        );
    }

    #[test]
    fn read_only() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();

        test_stage
            .add_system(empty_system)
            .add_system(empty_system)
            .add_system(res_system)
            .add_system(res_system)
            .add_system(nonsend_system)
            .add_system(nonsend_system)
            .add_system(read_component_system)
            .add_system(read_component_system)
            .add_system(event_reader_system)
            .add_system(event_reader_system)
            .add_system(read_world_system)
            .add_system(read_world_system);

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Forbid),
            0
        );
    }

    #[test]
    fn read_world() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();

        test_stage
            .add_system(empty_system)
            .add_system(resmut_system)
            .add_system(write_component_system)
            .add_system(event_writer_system)
            .add_system(read_world_system);

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Forbid),
            3
        );
    }

    #[test]
    fn resources() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();

        test_stage.add_system(resmut_system).add_system(res_system);

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Forbid),
            1
        );
    }

    #[test]
    fn nonsend() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();

        test_stage
            .add_system(nonsendmut_system)
            .add_system(nonsend_system);

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Forbid),
            1
        );
    }

    #[test]
    fn components() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();

        test_stage
            .add_system(read_component_system)
            .add_system(write_component_system);

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Forbid),
            1
        );
    }

    #[test]
    #[ignore = "Known failing but fix is non-trivial: https://github.com/bevyengine/bevy/issues/4381"]
    fn filtered_components() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();

        test_stage
            .add_system(with_filtered_component_system)
            .add_system(without_filtered_component_system);

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Forbid),
            0
        );
    }

    #[test]
    fn events() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();

        test_stage
            // All of these systems clash
            .add_system(event_reader_system)
            .add_system(event_writer_system)
            .add_system(event_resource_system);

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Forbid),
            3
        );
    }

    #[test]
    fn exclusive() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();

        test_stage
            // All 3 of these conflict with each other
            .add_system(write_world_system.exclusive_system())
            .add_system(write_world_system.exclusive_system().at_end())
            .add_system(res_system.exclusive_system())
            // These do not, as they're in different segments of the stage
            .add_system(write_world_system.exclusive_system().at_start())
            .add_system(write_world_system.exclusive_system().before_commands());

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Forbid),
            3
        );
    }

    // Tests for silencing and resolving ambiguities

    #[test]
    fn before_and_after() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();

        test_stage
            .add_system(event_reader_system.before(event_writer_system))
            .add_system(event_writer_system)
            .add_system(event_resource_system.after(event_writer_system));

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Forbid),
            // All of these systems clash
            0
        );
    }

    #[test]
    fn ignore_all_ambiguities() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();
        test_stage
            .add_system(resmut_system.ignore_all_ambiguities())
            .add_system(res_system);

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Warn),
            0
        );
    }

    #[test]
    fn ambiguous_with_label() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();
        test_stage
            .add_system(resmut_system.ambiguous_with("IGNORE_ME"))
            .add_system(res_system.label("IGNORE_ME"));

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Warn),
            0
        );
    }

    #[test]
    fn ambiguous_with_system() {
        let mut world = World::new();
        let mut test_stage = SystemStage::parallel();
        test_stage
            .add_system(system_a.ambiguous_with(system_b))
            .add_system(system_b);

        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Warn),
            0
        );
    }

    // Tests for reporting levels

    fn system_a(_res: ResMut<R>) {}
    fn system_b(_res: ResMut<R>) {}
    fn system_c(_res: ResMut<R>) {}
    fn system_d(_res: ResMut<R>) {}

    fn make_test_stage(world: &mut World) -> SystemStage {
        let mut test_stage = SystemStage::parallel();
        world.insert_resource(R);

        test_stage
            // Ambiguous with B and D
            .add_system(system_a)
            // Ambiguous with A
            .add_system(system_b.label("b"))
            .add_system(system_c.ignore_all_ambiguities())
            // Ambiguous with A
            .add_system(system_d.ambiguous_with("b"));

        test_stage
    }

    #[test]
    fn allow() {
        let mut world = World::new();
        let mut test_stage = make_test_stage(&mut world);
        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Allow),
            0
        );
    }

    #[test]
    fn warn() {
        let mut world = World::new();
        let mut test_stage = make_test_stage(&mut world);
        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Warn),
            2
        );
    }

    #[test]
    fn warn_verbose() {
        let mut world = World::new();
        let mut test_stage = make_test_stage(&mut world);
        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::WarnVerbose),
            2
        );
    }

    #[test]
    fn forbid() {
        let mut world = World::new();
        let mut test_stage = make_test_stage(&mut world);
        assert_eq!(
            test_stage.n_ambiguities(&mut world, ExecutionOrderAmbiguities::Forbid),
            6
        );
    }

    // Tests that the correct ambiguities were reported
    #[test]
    fn correct_ambiguities() {
        use crate::schedule::SystemOrderAmbiguity;
        use bevy_utils::HashSet;

        let mut world = World::new();
        let mut test_stage = make_test_stage(&mut world);
        let ambiguities =
            test_stage.ambiguities(&mut world, ExecutionOrderAmbiguities::WarnVerbose);
        assert_eq!(
            // We don't care if the reported order varies
            HashSet::from_iter(ambiguities),
            HashSet::from_iter(vec![
                SystemOrderAmbiguity {
                    system_names: [
                        "bevy_ecs::schedule::ambiguity_detection::tests::system_a".to_string(),
                        "bevy_ecs::schedule::ambiguity_detection::tests::system_b".to_string()
                    ],
                    conflicts: vec!["bevy_ecs::schedule::ambiguity_detection::tests::R".to_string()],
                    segment: bevy_ecs::schedule::SystemStageSegment::Parallel,
                },
                SystemOrderAmbiguity {
                    system_names: [
                        "bevy_ecs::schedule::ambiguity_detection::tests::system_a".to_string(),
                        "bevy_ecs::schedule::ambiguity_detection::tests::system_d".to_string()
                    ],
                    conflicts: vec!["bevy_ecs::schedule::ambiguity_detection::tests::R".to_string()],
                    segment: bevy_ecs::schedule::SystemStageSegment::Parallel,
                },
            ],)
        );
    }
}
