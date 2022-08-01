//! Components used to control the layout of [`UiNode`] entities.
use crate::{Size, UiRect, Val};
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::Component;
use bevy_reflect::prelude::*;
use serde::{Deserialize, Serialize};

/// Controls which layout algorithm is used to position this UI node
#[derive(
    Component, Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Reflect,
)]
#[reflect_value(PartialEq, Serialize, Deserialize)]
pub enum LayoutStrategy {
    /// Use the absolute position and size specified
    None,
    /// Use the [Flexbox](https://cssreference.io/flexbox/) layout algorithm
    ///
    /// As implemented by [`taffy`]: some bugs or limitations may exist; please file an issue!\
    #[default]
    Flex,
}

/// The strategy used to position this node
#[derive(
    Component, Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Reflect,
)]
#[reflect_value(PartialEq, Serialize, Deserialize)]
pub enum PositionType {
    /// Relative to all other nodes with the [`PositionType::Relative`] value
    #[default]
    Relative,
    /// Positioned as if it was the only child of its parent
    ///
    /// As usual, the `Style.position` field of this node is specified relative to its parent node
    Absolute,
}

/// The offset of a UI node from its base position
///
/// Layout is performed according to the [`LayoutStrategy`], and then this value is added at the end.
/// When this is [`LayoutStrategy::None`], this value will represent the absolute position of the UI node.
/// To check the final position of a UI element, read its [`Transform](bevy_transform::Transform) component.
#[derive(
    Component,
    Deref,
    DerefMut,
    Copy,
    Clone,
    PartialEq,
    Debug,
    Default,
    Serialize,
    Deserialize,
    Reflect,
)]
#[reflect_value(PartialEq, Serialize, Deserialize)]
pub struct Offset(pub UiRect<Val>);

/// Controls the size of UI nodes
///
/// Layout is performed according to the [`LayoutStrategy`]
/// To check the actual size of a UI element, read its [`Transform](bevy_transform::Transform) component
#[derive(Component, Copy, Clone, PartialEq, Debug, Default, Serialize, Deserialize, Reflect)]
#[reflect_value(PartialEq, Serialize, Deserialize)]
pub struct SizeConstraints {
    /// The minimum extent, which cannot be violated by the layouting algorithm
    pub min: Size<Val>,
    /// The suggested extent, which will be used if other constraints can be comfortably satisfied
    pub suggested: Size<Val>,
    /// The maximum extent, which cannot be violated by the layouting algorithm
    pub max: Size<Val>,
    /// The expected aspect ratio, computed as width / height
    pub aspect_ratio: Option<f32>,
}

impl SizeConstraints {
    ///```rust
    /// assert_eq!(SizeConstraints::DEFAULT, SizeConstraints::default())
    ///```
    pub const DEFAULT: SizeConstraints = SizeConstraints {
        min: Size::DEFAULT,
        suggested: Size::DEFAULT,
        max: Size::DEFAULT,
        aspect_ratio: None,
    };

    pub const FULL: SizeConstraints = SizeConstraints {
        min: Size::DEFAULT,
        suggested: Size::FULL,
        max: Size::FULL,
        aspect_ratio: None,
    };

    /// Sets only the minimum extent
    pub const fn min(width: Val, height: Val) -> SizeConstraints {
        SizeConstraints {
            min: Size::new(width, height),
            ..Self::DEFAULT
        }
    }

    /// Sets only the suggested extent
    pub const fn suggested(width: Val, height: Val) -> SizeConstraints {
        SizeConstraints {
            suggested: Size::new(width, height),
            ..Self::DEFAULT
        }
    }

    /// Sets only the suggested extent
    pub const fn max(width: Val, height: Val) -> SizeConstraints {
        SizeConstraints {
            max: Size::new(width, height),
            ..Self::DEFAULT
        }
    }
}

/// The space around and inside of a UI node
#[derive(Component, Copy, Clone, PartialEq, Debug, Default, Serialize, Deserialize, Reflect)]
#[reflect_value(PartialEq, Serialize, Deserialize)]
pub struct Spacing {
    /// The space around the outside of the UI element
    pub margin: UiRect<Val>,
    /// The space around the inside of the UI element
    pub padding: UiRect<Val>,
    /// The space around the outside of the UI element that can be colored to create a visible border
    pub border: UiRect<Val>,
}

impl Spacing {
    ///```rust
    /// assert_eq!(Spacing::DEFAULT, Spacing::default())
    ///```
    pub const DEFAULT: Spacing = Spacing {
        margin: UiRect::DEFAULT,
        padding: UiRect::DEFAULT,
        border: UiRect::DEFAULT,
    };

    /// Sets only the margin
    pub const fn margin(rect: UiRect<Val>) -> Spacing {
        Spacing {
            margin: rect,
            ..Self::DEFAULT
        }
    }

    /// Sets only the padding
    pub const fn padding(rect: UiRect<Val>) -> Spacing {
        Spacing {
            padding: rect,
            ..Self::DEFAULT
        }
    }

    /// Sets only the padding
    pub const fn border(rect: UiRect<Val>) -> Spacing {
        Spacing {
            border: rect,
            ..Self::DEFAULT
        }
    }
}

/// Defines the text direction
///
/// For example English is written LTR (left-to-right) while Arabic is written RTL (right-to-left).
#[derive(
    Component, Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Reflect,
)]
#[reflect_value(PartialEq, Serialize, Deserialize)]
pub enum TextDirection {
    /// Inherit from parent node
    #[default]
    Inherit,
    /// Text is written left to right
    LeftToRight,
    /// Text is written right to left
    RightToLeft,
}

/// Whether to show or hide overflowing items
#[derive(
    Component, Copy, Clone, PartialEq, Eq, Debug, Default, Reflect, Serialize, Deserialize,
)]
#[reflect_value(PartialEq, Serialize, Deserialize)]
pub enum Overflow {
    /// Show overflowing items
    #[default]
    Visible,
    /// Hide overflowing items
    Hidden,
}

/// Defines if child UI items appear on a single line or on multiple lines
#[derive(
    Component, Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Reflect,
)]
#[reflect_value(PartialEq, Serialize, Deserialize)]
pub enum Wrap {
    /// Single line, will overflow if needed
    #[default]
    NoWrap,
    /// Multiple lines, if needed
    Wrap,
    /// Same as [`FlexWrap::Wrap`] but new lines will appear before the previous one
    WrapReverse,
}

/// Flexbox-specific layout components
pub mod flex {
    use super::{
        LayoutStrategy, Offset, Overflow, PositionType, SizeConstraints, Spacing, TextDirection,
        Wrap,
    };
    use crate::Val;
    use bevy_ecs::prelude::Component;
    use bevy_ecs::query::{Changed, Or, WorldQuery};
    use bevy_reflect::prelude::*;
    use serde::{Deserialize, Serialize};

    /// A query for all of the components need for flexbox layout.
    ///
    /// See [`FlexLayoutChanged`] when attempting to use this as a query filter.
    #[derive(WorldQuery)]
    pub struct FlexLayoutQuery {
        /// The layout algorithm used
        pub layout_strategy: &'static LayoutStrategy,
        /// The position of this UI node
        pub offset: &'static Offset,
        /// Whether the node should be absolute or relatively positioned
        pub position_type: &'static PositionType,
        /// The constraints on the size of this node
        pub size_constraints: &'static SizeConstraints,
        /// The margin, padding and border of the UI node
        pub spacing: &'static Spacing,
        /// The flexbox layout parameters
        pub flex_layout: &'static FlexLayout,
        /// The direction of the text
        pub text_direction: &'static TextDirection,
        /// Controls how the content wraps
        pub wrap: &'static Wrap,
        /// The behavior in case the node overflows its allocated space
        pub overflow: &'static Overflow,
    }

    /// A type alias for when any of the components in a [`FlexLayoutQuery`] have changed.
    pub type FlexLayoutChanged = Or<(
        Changed<LayoutStrategy>,
        Changed<PositionType>,
        Changed<SizeConstraints>,
        Changed<Spacing>,
        Changed<FlexLayout>,
        Changed<TextDirection>,
        Changed<Overflow>,
    )>;

    /// The flexbox-specific layout configuration of a UI node
    ///
    /// This follows the web spec closely,
    /// you can use [guides](https://css-tricks.com/snippets/css/a-guide-to-flexbox/) for additional documentation.
    #[derive(Component, Serialize, Deserialize, Reflect, Debug, PartialEq, Clone, Copy)]
    #[reflect_value(PartialEq, Serialize, Deserialize)]
    pub struct FlexLayout {
        /// How items are ordered inside a flexbox
        ///
        /// Sets the main and cross-axis: if this is a "row" variant, the main axis will be rows.
        pub flex_direction: FlexDirection,
        /// Aligns this container's contents according to the cross-axis
        pub align_items: AlignItems,
        /// Overrides the inherited [`AlignItems`] value for this node
        pub align_self: AlignSelf,
        /// Aligns this containers lines according to the cross-axis
        pub align_content: AlignContent,
        /// Aligns this containers items along the main-axis
        pub justify_content: JustifyContent,
        /// Defines how much a flexbox item should grow if there's space available
        pub grow: f32,
        /// How to shrink if there's not enough space available
        pub shrink: f32,
        /// The initial size of the item
        pub basis: Val,
    }

    impl Default for FlexLayout {
        fn default() -> FlexLayout {
            FlexLayout {
                flex_direction: Default::default(),
                align_items: Default::default(),
                align_self: Default::default(),
                align_content: Default::default(),
                justify_content: Default::default(),
                grow: 0.0,
                shrink: 1.0,
                basis: Val::Auto,
            }
        }
    }

    /// Defines how flexbox items are ordered within a flexbox
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Reflect)]
    #[reflect_value(PartialEq, Serialize, Deserialize)]
    pub enum FlexDirection {
        /// Same way as text direction along the main axis
        #[default]
        Row,
        /// Flex from bottom to top
        Column,
        /// Opposite way as text direction along the main axis
        RowReverse,
        /// Flex from top to bottom
        ColumnReverse,
    }

    /// How items are aligned according to the cross axis
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Reflect)]
    #[reflect_value(PartialEq, Serialize, Deserialize)]
    pub enum AlignItems {
        /// Items are aligned at the start
        FlexStart,
        /// Items are aligned at the end
        FlexEnd,
        /// Items are aligned at the center
        Center,
        /// Items are aligned at the baseline
        Baseline,
        /// Items are stretched across the whole cross axis
        #[default]
        Stretch,
    }

    /// Works like [`AlignItems`] but applies only to a single item
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Reflect)]
    #[reflect_value(PartialEq, Serialize, Deserialize)]
    pub enum AlignSelf {
        /// Use the value of [`AlignItems`]
        #[default]
        Auto,
        /// If the parent has [`AlignItems::Center`] only this item will be at the start
        FlexStart,
        /// If the parent has [`AlignItems::Center`] only this item will be at the end
        FlexEnd,
        /// If the parent has [`AlignItems::FlexStart`] only this item will be at the center
        Center,
        /// If the parent has [`AlignItems::Center`] only this item will be at the baseline
        Baseline,
        /// If the parent has [`AlignItems::Center`] only this item will stretch along the whole cross axis
        Stretch,
    }

    /// Defines how each line is aligned within the flexbox.
    ///
    /// It only applies if [`FlexWrap::Wrap`] is present and if there are multiple lines of items.
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Reflect)]
    #[reflect_value(PartialEq, Serialize, Deserialize)]
    pub enum AlignContent {
        /// Each line moves towards the start of the cross axis
        FlexStart,
        /// Each line moves towards the end of the cross axis
        FlexEnd,
        /// Each line moves towards the center of the cross axis
        Center,
        /// Each line will stretch to fill the remaining space
        #[default]
        Stretch,
        /// Each line fills the space it needs, putting the remaining space, if any
        /// inbetween the lines
        SpaceBetween,
        /// Each line fills the space it needs, putting the remaining space, if any
        /// around the lines
        SpaceAround,
    }

    /// Defines how items are aligned according to the main axis
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Reflect)]
    #[reflect_value(PartialEq, Serialize, Deserialize)]
    pub enum JustifyContent {
        /// Pushed towards the start
        #[default]
        FlexStart,
        /// Pushed towards the end
        FlexEnd,
        /// Centered along the main axis
        Center,
        /// Remaining space is distributed between the items
        SpaceBetween,
        /// Remaining space is distributed around the items
        SpaceAround,
        /// Like [`JustifyContent::SpaceAround`] but with even spacing between items
        SpaceEvenly,
    }
}
