#![warn(missing_docs)]

//! Components used to control the layout of [`UiNode`] entities.
use crate::{Size, UiRect, Val};
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::{Bundle, Component};
use bevy_reflect::prelude::*;
use serde::{Deserialize, Serialize};

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
    Flexbox,
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
    /// Independent of all other nodes
    ///
    /// As usual, the `Style.position` field of this node is specified relative to its parent node
    Absolute,
}

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
pub struct UiPosition(pub UiRect<Val>);

#[derive(Component, Copy, Clone, PartialEq, Debug, Default, Serialize, Deserialize, Reflect)]
#[reflect_value(PartialEq, Serialize, Deserialize)]
pub struct SizeConstraints {
    pub min: Size<Val>,
    pub suggested: Size<Val>,
    pub max: Size<Val>,
}

#[derive(Component, Copy, Clone, PartialEq, Debug, Default, Serialize, Deserialize, Reflect)]
#[reflect_value(PartialEq, Serialize, Deserialize)]
pub struct Decorations {
    pub margin: UiRect<Val>,
    pub padding: UiRect<Val>,
    pub border: UiRect<Val>,
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

pub mod flex {
    use super::*;

    /// A [`Bundle`] used to control the layout of a UI node
    #[derive(Bundle)]
    pub struct FlexboxLayoutBundle {
        /// The layout algorithm used
        pub layout_strategy: LayoutStrategy,
        /// The position of this UI node
        pub position: UiPosition,
        /// Whether the node should be absolute or relatively positioned
        pub position_type: PositionType,
        /// The constraints on the size of this node
        pub size_constraints: SizeConstraints,
        /// The margin, padding and border of the UI node
        pub decorations: Decorations,
        /// The flexbox layout parameters
        pub flexbox_layout: FlexboxLayout,
        /// The direction of the text
        pub text_direction: TextDirection,
        /// The behavior in case the node overflows its allocated space
        pub overflow: Overflow,
    }

    #[derive(Component, Serialize, Deserialize, Reflect, Debug, PartialEq, Clone, Copy)]
    #[reflect_value(PartialEq, Serialize, Deserialize)]
    pub struct FlexboxLayout {
        pub flex_direction: FlexDirection,
        pub align_items: AlignItems,
        pub align_self: AlignSelf,
        pub align_content: AlignContent,
        pub justify_content: JustifyContent,
        pub flex_wrap: FlexWrap,
        /// Defines how much a flexbox item should grow if there's space available
        pub flex_grow: f32,
        /// How to shrink if there's not enough space available
        pub flex_shrink: f32,
        /// The initial size of the item
        pub flex_basis: Val,
        /// The aspect ratio of the flexbox
        pub aspect_ratio: Option<f32>,
    }

    impl Default for FlexboxLayout {
        fn default() -> FlexboxLayout {
            FlexboxLayout {
                flex_direction: Default::default(),
                align_items: Default::default(),
                align_self: Default::default(),
                align_content: Default::default(),
                justify_content: Default::default(),
                flex_wrap: Default::default(),
                flex_grow: 0.0,
                flex_shrink: 1.0,
                flex_basis: Val::Auto,
                aspect_ratio: Default::default(),
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

    /// Defines if flexbox items appear on a single line or on multiple lines
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Reflect)]
    #[reflect_value(PartialEq, Serialize, Deserialize)]
    pub enum FlexWrap {
        /// Single line, will overflow if needed
        #[default]
        NoWrap,
        /// Multiple lines, if needed
        Wrap,
        /// Same as [`FlexWrap::Wrap`] but new lines will appear before the previous one
        WrapReverse,
    }
}
