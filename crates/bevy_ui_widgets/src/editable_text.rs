//! A simple text input widget for Bevy UI.
//!
//! The [`EditableText`] widget is an undecorated rectangular text input field,
//! which allows users to input and edit text within a Bevy UI application.
//! Every [`EditableText`] component is also a [`Node`] in the Bevy UI hierarchy,
//! allowing you to position and size it using standard Bevy UI layout techniques.
//! You can think of it as the editable equivalent of [`Text`](bevy_ui::prelude::Text),
//! and components such as [`TextFont`] and [`TextColor`] can be used to style it.
//!
//! [`EditableText`] supports the following functionality:
//!
//! - Text entry
//! - Basic keyboard-driven cursor movement (arrow keys, home/end keys)
//! - Backspace and delete operations
//!
//! You might use this widget as the basis for text input fields in forms, chat boxes, for naming characters,
//! or any other scenario where you want to extract an unformatted text string from the user.
//!
//! Reusable widgets that build on top of this basic text input field (as might be found in Bevy's Feathers UI framework),
//! will typically combine this widget with additional UI elements such as borders, backgrounds, and labels,
//! creating a multi-entity widget that matches the semantics and visual appearance required by the application.
//!
//! ## Handling user input
//!
//! When an [`EditableText`] entity is focused (see [`InputFocus`]),
//! keyboard input events are captured and processed into [`TextEdit`] actions.
//! This is done by the [`process_text_inputs`] system, which is included in [`EditableTextPlugin`],
//! which itself can be added via the [`UiWidgetsPlugins`](crate::UiWidgetsPlugins) plugin group.
//!
//! ## Limitations
//!
//! The formatting of the text is uniform throughout the entire input field.
//! As a result, rich text-editing is out-of-scope:
//! this widget is not intended to form the basis for a full-featured text editor.
//!
//! Similarly, this widget is "headless": it has no built-in styling, and is intended to be used
//! with a themed UI framework of your choice (e.g. Feathers). This means that no text boxes, borders, or other
//! visual elements are provided by default, and must be added separately using Bevy UI entities / components,
//! and any reactive styling (e.g., focus/hover states) must also be implemented separately.
//!
//! However, the following features are planned but currently not implemented:
//!
//! - Home / End key support for moving the cursor to the start / end of the text
//! - Placeholder text (displayed when the input is empty)
//! - Click to place cursor
//! - Cursor blinking
//! - Clipboard operations (copy, cut, paste)
//! - Text selection
//! - Undo/redo functionality
//! - Newline support for multi-line input
//! - Input Method Editor (IME) support for complex scripts
//! - Text validation (e.g., email format, numeric input, max length)
//! - Password-style character masking
//! - Vertical scrolling for multi-line input
//! - Horizontal scrolling for long lines
//! - Mobile pop-up keyboard support
//! - Overwrite mode (typically toggled by the `Insert` key)
//! - Bidirectional text support (e.g., mixing left-to-right and right-to-left scripts)
//! - AccessKit integration for screen readers and other assistive technologies
//! - World-space text input
//! - Text input labels (used for accessibility, tooltips or form descriptions)
//! - Input consumption (preventing other systems from receiving keyboard input events when the text input is focused)
//! - Text form submission handling
//!
//! If you require any of these features, please consider contributing it to the crate,
//! one feature at a time!
//!
//! # Usage
//!
//! To use this widget, ensure that the [`EditableTextPlugin`] has been added to your Bevy app.
//! Then, you can add a [`EditableText`] component to any UI node.

use std::collections::VecDeque;

use bevy_app::{App, Plugin, PostUpdate, PreUpdate};
use bevy_ecs::prelude::*;
use bevy_input::keyboard::{Key, KeyboardInput};
use bevy_input::InputSystems;
use bevy_input_focus::{InputFocus, InputFocusSystems};
use bevy_reflect::prelude::*;
use bevy_text::{FontHinting, LineHeight, TextColor, TextFont, TextLayout};
use bevy_ui::{widget::TextNodeFlags, ContentSize, Node, UiSystems};
use smol_str::SmolStr;

/// A plain-text text input field.
///
/// Please see the [`editable_text` module](crate::editable_text) for more details on usage and functionality.
///
/// Note that text editing operations are trickier than they might first appear,
/// due to the complexities of Unicode text handling.
///
/// As a result, you should prefer to use the editing operations exposed by the methods on this type,
/// rather than manipulating the `current_input` and `cursor_index` fields directly.
#[derive(Component, Debug, Default, Reflect)]
#[reflect(Component, Default)]
#[require(
    Node,
    TextLayout,
    TextFont,
    TextColor,
    LineHeight,
    TextNodeFlags,
    ContentSize,
    FontHinting
)]
pub struct EditableText {
    /// The text that has been input.
    pub current_input: String,
    /// The index of the cursor within the text.
    ///
    /// Note: this is a direct index into the [`String`], and does not correspond directly to a character position.
    /// Unicode is complicated!
    pub cursor_index: usize,
    /// Text edit actions that have been requested but not yet applied.
    ///
    /// These edits are processed in first-in, first-out order.
    pub pending_edits: VecDeque<TextEdit>,
}

impl EditableText {
    /// Creates a new [`EditableText`] component with default settings.
    ///
    /// Equivalent to [`EditableText::default()`].
    pub const fn new() -> Self {
        Self {
            current_input: String::new(),
            cursor_index: 0,
            pending_edits: VecDeque::new(),
        }
    }

    /// Inserts the given text at the current cursor position.
    ///
    /// This is also used for [`TextEdit::Insert`], which does not assume that each keyboard input corresponds to a single [`char`] byte.
    pub fn insert_text(&mut self, text: &str) {
        self.current_input.insert_str(self.cursor_index, text);
        self.cursor_index += text.len();
    }

    /// Deletes the character before the cursor.
    pub fn backspace(&mut self) {
        if self.cursor_index > 0 {
            let new_index = self.current_input.floor_char_boundary(self.cursor_index);

            for i in new_index..self.cursor_index {
                self.current_input.remove(i);
            }

            self.cursor_index = new_index;
        }
    }

    /// Deletes the character at the cursor.
    pub fn delete(&mut self) {
        if self.cursor_index < self.current_input.len() {
            for i in self.cursor_index..self.current_input.ceil_char_boundary(self.cursor_index) {
                self.current_input.remove(i);
            }
        }
    }

    /// Moves the cursor one position to the right.
    pub fn move_cursor_right(&mut self) {
        if self.cursor_index < self.current_input.len() {
            self.cursor_index = self.current_input.ceil_char_boundary(self.cursor_index);
        }
    }

    /// Moves the cursor one position to the left.
    pub fn move_cursor_left(&mut self) {
        if self.cursor_index > 0 {
            self.cursor_index = self.current_input.floor_char_boundary(self.cursor_index);
        }
    }

    /// Clears the current input and resets the cursor position.
    pub fn clear(&mut self) {
        self.current_input.clear();
        self.cursor_index = 0;
    }
}

/// Deferred text input edit and navigation actions applied by the `apply_text_edits` system.
#[derive(Debug, Clone, Reflect, PartialEq, Eq)]
pub enum TextEdit {
    /// Insert a character at the cursor. If there is a selection, replaces the selection with the character instead.
    ///
    /// Typically generated in response to keyboard text input events.
    ///
    /// This is intended to insert a single Unicode grapheme cluster, such as a letter, digit, punctuation mark, or emoji.
    /// Ordinarily, this is derived from [`KeyboardInput::logical_key`](bevy_input::keyboard::KeyboardInput::logical_key),
    /// which stores a [`SmolStr`] inside of the [`Key::Character`] variant, which may represent multiple bytes.
    Insert(SmolStr),
    /// Delete the character behind the cursor.
    /// If there is a selection, deletes the selection instead.
    ///
    /// Typically generated in response to the [`Backspace`](Key::Backspace) key.
    ///
    /// This operation removes an entire Unicode grapheme cluster, which may consist of multiple bytes,
    /// shifting the cursor position accordingly.
    Backspace,
    /// Delete the character at the cursor.
    /// If there is a selection, deletes the selection instead.
    ///
    /// Typically generated in response to the [`Delete`](Key::Delete) key.
    ///
    /// This operation removes an entire Unicode grapheme cluster, which may consist of multiple bytes,
    /// shifting the cursor position accordingly.
    Delete,
    /// Clear the text input buffer.
    ///
    /// Typically programmatically generated, triggered by UI actions such as pressing a "Clear" button.
    ///
    /// This action is not generated or handled in by [this crate](crate), but is provided as a standardized
    /// hook for external systems.
    Clear,
    /// Moves the cursor by one position to the right.
    ///
    /// Typically generated in response to the [`Right`](Key::Right) key.
    MoveCursorRight,
    /// Moves the cursor by one position to the left.
    ///
    /// Typically generated in response to the [`Left`](Key::Left) key.
    MoveCursorLeft,
}

/// System that processes keyboard input events into text edit actions for focused [`EditableText`] widgets.
///
/// See [`EditableText`] for more details on the standard mapping from keyboard events to text edit actions
/// used by this system.
///
/// Note that this does not immediately apply the edits; they are queued up in [`EditableText::pending_edits`],
/// and then applied later by the [`apply_text_edits`] system.
pub fn process_text_inputs(
    focus: Res<InputFocus>,
    mut query: Query<&mut EditableText>,
    mut keyboard_input: MessageReader<KeyboardInput>,
) {
    // Check if any EditableText is focused
    let focused_entity = if let Some(entity) = focus.get() {
        entity
    } else {
        return; // No focused entity, nothing to do
    };

    let mut editable_text = if let Ok(editable_text) = query.get_mut(focused_entity) {
        editable_text
    } else {
        return; // Focused entity is not an EditableText, nothing to do
    };

    for keyboard_event in keyboard_input.read() {
        match keyboard_event {
            KeyboardInput {
                logical_key: Key::Character(c),
                state: bevy_input::ButtonState::Pressed,
                ..
            } => {
                editable_text
                    .pending_edits
                    .push_back(TextEdit::Insert(c.clone()));
            }
            KeyboardInput {
                logical_key: Key::Backspace,
                state: bevy_input::ButtonState::Pressed,
                ..
            } => {
                editable_text.pending_edits.push_back(TextEdit::Backspace);
            }
            KeyboardInput {
                logical_key: Key::Delete,
                state: bevy_input::ButtonState::Pressed,
                ..
            } => {
                editable_text.pending_edits.push_back(TextEdit::Delete);
            }
            KeyboardInput {
                logical_key: Key::ArrowRight,
                state: bevy_input::ButtonState::Pressed,
                ..
            } => {
                editable_text
                    .pending_edits
                    .push_back(TextEdit::MoveCursorRight);
            }
            KeyboardInput {
                logical_key: Key::ArrowLeft,
                state: bevy_input::ButtonState::Pressed,
                ..
            } => {
                editable_text
                    .pending_edits
                    .push_back(TextEdit::MoveCursorLeft);
            }
            _ => {}
        }
    }
}

/// Applies pending text edit actions to all [`EditableText`] widgets.
pub fn apply_text_edits(mut query: Query<&mut EditableText>) {
    for mut editable_text in query.iter_mut() {
        while let Some(edit) = editable_text.pending_edits.pop_front() {
            match edit {
                TextEdit::Insert(str) => editable_text.insert_text(&str),
                TextEdit::Backspace => editable_text.backspace(),
                TextEdit::Delete => editable_text.delete(),
                TextEdit::Clear => editable_text.clear(),
                TextEdit::MoveCursorRight => editable_text.move_cursor_right(),
                TextEdit::MoveCursorLeft => editable_text.move_cursor_left(),
            }
        }
    }
}

/// Enables support for the [`EditableText`] widget.
///
/// Contains the systems and observers necessary to update widget state and handle user input.
pub struct EditableTextPlugin;

impl Plugin for EditableTextPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            process_text_inputs
                .after(InputFocusSystems::Dispatch)
                .after(InputSystems),
        )
        .add_systems(PostUpdate, apply_text_edits.in_set(UiSystems::Prepare));
    }
}
