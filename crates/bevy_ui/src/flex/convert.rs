use crate::layout_components::{
    flex::{
        AlignContent, AlignItems, AlignSelf, FlexDirection, FlexLayoutQueryItem, JustifyContent,
    },
    LayoutStrategy, PositionType, Wrap,
};
use crate::{Size, UiRect, Val};

pub fn from_rect(
    scale_factor: f64,
    rect: UiRect,
) -> taffy::geometry::Rect<taffy::style::Dimension> {
    taffy::geometry::Rect {
        start: from_val(scale_factor, rect.left),
        end: from_val(scale_factor, rect.right),
        // NOTE: top and bottom are intentionally flipped. stretch has a flipped y-axis
        top: from_val(scale_factor, rect.bottom),
        bottom: from_val(scale_factor, rect.top),
    }
}

pub fn from_f32_size(scale_factor: f64, size: Size) -> taffy::geometry::Size<f32> {
    taffy::geometry::Size {
        width: val_to_f32(scale_factor, size.width),
        height: val_to_f32(scale_factor, size.height),
    }
}

pub fn from_val_size(
    scale_factor: f64,
    size: Size,
) -> taffy::geometry::Size<taffy::style::Dimension> {
    taffy::geometry::Size {
        width: from_val(scale_factor, size.width),
        height: from_val(scale_factor, size.height),
    }
}

pub fn from_flex_layout(scale_factor: f64, value: FlexLayoutQueryItem<'_>) -> taffy::style::Style {
    taffy::style::Style {
        display: (*value.layout_strategy).into(),
        position_type: (*value.position_type).into(),
        flex_direction: value.flex_layout.flex_direction.into(),
        flex_wrap: (*value.wrap).into(),
        align_items: value.flex_layout.align_items.into(),
        align_self: value.flex_layout.align_self.into(),
        align_content: value.flex_layout.align_content.into(),
        justify_content: value.flex_layout.justify_content.into(),
        position: from_rect(scale_factor, value.offset.0),
        margin: from_rect(scale_factor, value.spacing.margin),
        padding: from_rect(scale_factor, value.spacing.padding),
        border: from_rect(scale_factor, value.spacing.border),
        flex_grow: value.flex_layout.grow,
        flex_shrink: value.flex_layout.shrink,
        flex_basis: from_val(scale_factor, value.flex_layout.basis),
        size: from_val_size(scale_factor, value.size_constraints.suggested),
        min_size: from_val_size(scale_factor, value.size_constraints.min),
        max_size: from_val_size(scale_factor, value.size_constraints.max),
        aspect_ratio: match value.size_constraints.aspect_ratio {
            Some(value) => taffy::number::Number::Defined(value),
            None => taffy::number::Number::Undefined,
        },
    }
}

/// Converts a [`Val`] to a [`f32`] while respecting the scale factor.
pub fn val_to_f32(scale_factor: f64, val: Val) -> f32 {
    match val {
        Val::Undefined | Val::Auto => 0.0,
        Val::Px(value) => (scale_factor * value as f64) as f32,
        Val::Percent(value) => value / 100.0,
    }
}

pub fn from_val(scale_factor: f64, val: Val) -> taffy::style::Dimension {
    match val {
        Val::Auto => taffy::style::Dimension::Auto,
        Val::Percent(value) => taffy::style::Dimension::Percent(value / 100.0),
        Val::Px(value) => taffy::style::Dimension::Points((scale_factor * value as f64) as f32),
        Val::Undefined => taffy::style::Dimension::Undefined,
    }
}

impl From<AlignItems> for taffy::style::AlignItems {
    fn from(value: AlignItems) -> Self {
        match value {
            AlignItems::FlexStart => taffy::style::AlignItems::FlexStart,
            AlignItems::FlexEnd => taffy::style::AlignItems::FlexEnd,
            AlignItems::Center => taffy::style::AlignItems::Center,
            AlignItems::Baseline => taffy::style::AlignItems::Baseline,
            AlignItems::Stretch => taffy::style::AlignItems::Stretch,
        }
    }
}

impl From<AlignSelf> for taffy::style::AlignSelf {
    fn from(value: AlignSelf) -> Self {
        match value {
            AlignSelf::Auto => taffy::style::AlignSelf::Auto,
            AlignSelf::FlexStart => taffy::style::AlignSelf::FlexStart,
            AlignSelf::FlexEnd => taffy::style::AlignSelf::FlexEnd,
            AlignSelf::Center => taffy::style::AlignSelf::Center,
            AlignSelf::Baseline => taffy::style::AlignSelf::Baseline,
            AlignSelf::Stretch => taffy::style::AlignSelf::Stretch,
        }
    }
}

impl From<AlignContent> for taffy::style::AlignContent {
    fn from(value: AlignContent) -> Self {
        match value {
            AlignContent::FlexStart => taffy::style::AlignContent::FlexStart,
            AlignContent::FlexEnd => taffy::style::AlignContent::FlexEnd,
            AlignContent::Center => taffy::style::AlignContent::Center,
            AlignContent::Stretch => taffy::style::AlignContent::Stretch,
            AlignContent::SpaceBetween => taffy::style::AlignContent::SpaceBetween,
            AlignContent::SpaceAround => taffy::style::AlignContent::SpaceAround,
        }
    }
}

impl From<LayoutStrategy> for taffy::style::Display {
    fn from(value: LayoutStrategy) -> Self {
        match value {
            LayoutStrategy::Flex => taffy::style::Display::Flex,
            LayoutStrategy::None => taffy::style::Display::None,
        }
    }
}

impl From<FlexDirection> for taffy::style::FlexDirection {
    fn from(value: FlexDirection) -> Self {
        match value {
            FlexDirection::Row => taffy::style::FlexDirection::Row,
            FlexDirection::Column => taffy::style::FlexDirection::Column,
            FlexDirection::RowReverse => taffy::style::FlexDirection::RowReverse,
            FlexDirection::ColumnReverse => taffy::style::FlexDirection::ColumnReverse,
        }
    }
}

impl From<JustifyContent> for taffy::style::JustifyContent {
    fn from(value: JustifyContent) -> Self {
        match value {
            JustifyContent::FlexStart => taffy::style::JustifyContent::FlexStart,
            JustifyContent::FlexEnd => taffy::style::JustifyContent::FlexEnd,
            JustifyContent::Center => taffy::style::JustifyContent::Center,
            JustifyContent::SpaceBetween => taffy::style::JustifyContent::SpaceBetween,
            JustifyContent::SpaceAround => taffy::style::JustifyContent::SpaceAround,
            JustifyContent::SpaceEvenly => taffy::style::JustifyContent::SpaceEvenly,
        }
    }
}

impl From<PositionType> for taffy::style::PositionType {
    fn from(value: PositionType) -> Self {
        match value {
            PositionType::Relative => taffy::style::PositionType::Relative,
            PositionType::Absolute => taffy::style::PositionType::Absolute,
        }
    }
}

impl From<Wrap> for taffy::style::FlexWrap {
    fn from(value: Wrap) -> Self {
        match value {
            Wrap::NoWrap => taffy::style::FlexWrap::NoWrap,
            Wrap::Wrap => taffy::style::FlexWrap::Wrap,
            Wrap::WrapReverse => taffy::style::FlexWrap::WrapReverse,
        }
    }
}
