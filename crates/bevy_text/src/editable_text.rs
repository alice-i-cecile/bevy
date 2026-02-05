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
//! User input is handled via a plugin in `bevy_ui_widgets`:
//! [`bevy_text`](crate) is not aware of input events directly.
//!
//! With the correct plugin enabled, when an [`EditableText`] entity is focused,
//! keyboard input events are captured and processed into [`TextEdit`] actions.
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
//! - Soft-wrapping of long lines
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
// Note: this logic is in `bevy_text`, rather than higher up in `bevy_ui` or `bevy_ui_widgets`,
// because doing so allows us to process `EditableText` in the various systems provided by `bevy_text`
// and `bevy_ui`, such as text layout and font management.

use std::collections::VecDeque;

use crate::{CosmicFontSystem, FontHinting, LineHeight, TextColor, TextFont, TextLayout};
use bevy_ecs::prelude::*;
use cosmic_text::{Action, Buffer, BufferRef, Edit, Editor, FontSystem, Metrics, Motion};
use smol_str::SmolStr;

/// A plain-text text input field.
///
/// Please see the [`editable_text` module](crate::editable_text) for more details on usage and functionality.
///
/// Note that text editing operations are trickier than they might first appear,
/// due to the complexities of Unicode text handling.
///
/// As a result, we store an internal [`cosmic_text::Editor`] instance,
/// which manages both the text content and the cursor position,
/// and provides methods for applying text edits and cursor movements correctly
/// according to Unicode rules.
#[derive(Component, Debug)]
#[require(TextLayout, TextFont, TextColor, LineHeight, FontHinting)]
pub struct EditableText {
    /// A [cosmic_text::Editor], tracking both the text content and cursor position.
    ///
    /// This stores an owned [`Buffer`] with a 'static` lifetime, as Bevy ECS components must be `'static`.
    /// This also stores a [`Cursor`](cosmic_text::Cursor) internally.
    ///
    /// This serves as an analogue to [`ComputedTextBlock`](crate::ComputedTextBlock) for editable text,
    /// We cannot simply hold a BufferRef::Borrowed pointing to the [`Buffer`] inside of
    /// that component, because that would require a non-'static lifetime,
    /// making this type unusable as a component.
    pub editor: Editor<'static>,
    /// Text edit actions that have been requested but not yet applied.
    ///
    /// These edits are processed in first-in, first-out order.
    pub pending_edits: VecDeque<TextEdit>,
    /// Does the contained text buffer need rerendering / relayout?
    ///
    /// Analogous to [`ComputedTextBlock::needs_rerender`](crate::ComputedTextBlock::needs_rerender).
    pub needs_rerender: bool,
}

impl Default for EditableText {
    fn default() -> Self {
        let buffer = Buffer::new(&mut FontSystem::new(), Metrics::new(20.0, 20.0));

        Self {
            // Defaults selected to match `Text::default()`
            editor: Editor::new(BufferRef::Owned(buffer)),
            pending_edits: VecDeque::new(),
            needs_rerender: true,
        }
    }
}

impl EditableText {
    /// Access the internal [`cosmic_text::Buffer`].
    pub fn buffer(&self) -> &Buffer {
        let buffer_ref = self.editor.buffer_ref();
        let BufferRef::Owned(buffer) = buffer_ref else {
            panic!("EditableText editor buffer_ref is not Owned");
        };
        buffer
    }

    /// Mutably access the internal [`cosmic_text::Buffer`].
    pub fn buffer_mut(&mut self) -> &mut Buffer {
        let buffer_ref = self.editor.buffer_ref_mut();
        let BufferRef::Owned(buffer) = buffer_ref else {
            panic!("EditableText editor buffer_ref is not Owned");
        };
        buffer
    }

    /// Get the current text input as a [`String`].
    ///
    /// This allocates, as we must combine the internal representation into a single string.
    pub fn value(&self) -> String {
        let mut combined_string = String::new();

        for line in &self.buffer().lines {
            // Combine the lines into a single string,
            // adding line breaks between each line.
            if !combined_string.is_empty() {
                combined_string.push('\n');
            }
            combined_string.push_str(&line.text());
        }

        combined_string
    }

    /// Inserts the given string (usually one character) at the current cursor position.
    ///
    /// This is also used for [`TextEdit::Insert`], which does not assume that each keyboard input corresponds to a single [`char`] byte.
    pub fn insert_text(&mut self, text: &str) {
        let new_cursor = self.editor.insert_at(self.editor.cursor(), text, None);
        self.editor.set_cursor(new_cursor);
    }

    /// Queue a [`TextEdit`] action to be applied later by the [`apply_text_edits`] system.
    pub fn queue_edit(&mut self, edit: TextEdit) {
        self.pending_edits.push_back(edit);
    }

    /// Sets the entire text input to the given string, replacing any existing content.
    pub fn set_input(&mut self, text: &str, font_system: &mut FontSystem) {
        self.clear(font_system);
        self.insert_text(text);
    }

    /// Applies a [`cosmic_text::Motion`] to the cursor.
    ///
    /// This includes operations such as moving left/right, to start/end of line, etc.
    pub fn apply_cursor_motion(&mut self, motion: Motion, font_system: &mut FontSystem) {
        let cursor = self.editor.cursor();

        let output = self
            .buffer_mut()
            .cursor_motion(font_system, cursor, None, motion);
        if let Some((new_cursor, _scroll_to)) = output {
            self.editor.set_cursor(new_cursor);
        }
    }

    /// Applies a [`cosmic_text::Action`] editing operation at the current cursor position.
    ///
    /// This includes operations such as backspace, delete, etc.
    pub fn apply_edit(&mut self, edit: Action, font_system: &mut FontSystem) {
        self.editor.action(font_system, edit);
    }

    /// Deletes the character before the cursor.
    pub fn backspace(&mut self, font_system: &mut FontSystem) {
        self.apply_edit(Action::Backspace, font_system);
    }

    /// Deletes the character at the cursor.
    pub fn delete(&mut self, font_system: &mut FontSystem) {
        self.apply_edit(Action::Delete, font_system);
    }

    /// Moves the cursor one position to the right.
    pub fn move_cursor_right(&mut self, font_system: &mut FontSystem) {
        self.apply_cursor_motion(Motion::Right, font_system);
    }

    /// Moves the cursor one position to the left.
    pub fn move_cursor_left(&mut self, font_system: &mut FontSystem) {
        self.apply_cursor_motion(Motion::Left, font_system);
    }

    /// Clears the current input and resets the cursor position.
    pub fn clear(&mut self, font_system: &mut FontSystem) {
        self.editor = Editor::new(BufferRef::Owned(Buffer::new(
            font_system,
            Metrics::new(20.0, 20.0),
        )));
    }
}

/// Deferred text input edit and navigation actions applied by the `apply_text_edits` system.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Applies pending text edit actions to all [`EditableText`] widgets.
pub fn apply_text_edits(
    mut query: Query<&mut EditableText>,
    mut font_system: ResMut<CosmicFontSystem>,
) {
    for mut editable_text in query.iter_mut() {
        while let Some(edit) = editable_text.pending_edits.pop_front() {
            match edit {
                TextEdit::Insert(str) => editable_text.insert_text(&str),
                TextEdit::Backspace => editable_text.backspace(&mut font_system.0),
                TextEdit::Delete => editable_text.delete(&mut font_system.0),
                TextEdit::Clear => editable_text.clear(&mut font_system.0),
                TextEdit::MoveCursorRight => editable_text.move_cursor_right(&mut font_system.0),
                TextEdit::MoveCursorLeft => editable_text.move_cursor_left(&mut font_system.0),
            }
        }
    }
}
