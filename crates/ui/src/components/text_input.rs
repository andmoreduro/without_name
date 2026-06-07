use std::ops::Range;

use gpui::{
    Bounds, ClipboardItem, Context, Element, Entity, EntityInputHandler, FocusHandle, IntoElement,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine,
    SharedString, UTF16Selection, Window, actions, point,
};
use unicode_segmentation::*;

actions!(
    input,
    [
        Backspace,
        Clear,
        Copy,
        Cut,
        Delete,
        End,
        Left,
        Paste,
        Right,
        SelectAll,
        SelectLeft,
        SelectRight,
        Start,
    ]
);

struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range_utf8: Range<usize>,
    selection_reversed: bool,
    marked_range_utf8: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

impl TextInput {
    /// Moves the cursor to the previous grapheme boundary and removes selection.
    fn left(&mut self, _: &Left, cx: &mut Context<Self>) {
        self.move_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    /// Moves the cursor to the next grapheme boundary and removes selection.
    fn right(&mut self, _: &Right, cx: &mut Context<Self>) {
        self.move_to(self.next_boundary(self.cursor_offset()), cx);
    }

    /// Moves the cursor to the start of the content.
    fn start(&mut self, _: &Start, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    /// Moves the cursor to the end of the content.
    fn end(&mut self, _: &End, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    /// Moves the start of the selection to the previous grapheme boundary.
    fn select_left(&mut self, _: &SelectLeft, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    /// Moves the start of the selection to the next grapheme boundary.
    fn select_right(&mut self, _: &SelectRight, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    /// Selects all content, leaving the cursor at the start.
    fn select_all(&mut self, _: &SelectAll, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    /// Adds the selected range to the system clipboard.
    fn copy(&mut self, _: &Copy, cx: &mut Context<Self>) {
        if !self.selected_range_utf8.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range_utf8.clone()].to_string(),
            ));
        }
    }

    /// Replaces the contents inside the selected range for the clipboard's content while replacing newlines for spaces.
    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace("\n", " "), window, cx);
        }
    }

    /// Adds the selected range to the system clipboard and deletes the content inside the former.
    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range_utf8.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range_utf8.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    /// Removes the grapheme to the left.
    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range_utf8.is_empty() {
            let previous_boundary = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == previous_boundary {
                return;
            }
            self.select_to(previous_boundary, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    /// Removes the grapheme to the right.
    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range_utf8.is_empty() {
            let next_boundary = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next_boundary {
                return;
            }
            self.select_to(next_boundary, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    /// Fully clears the input.
    fn clear(&mut self, _: &Clear, cx: &mut Context<Self>) {
        self.content = "".into();
        self.selected_range_utf8 = 0..0;
        self.selection_reversed = false;
        self.marked_range_utf8 = None;
        self.last_layout = None;
        self.last_bounds = None;
        self.is_selecting = false;
        cx.notify();
    }

    /// Activates mouse selection, extends selection if holding shift to the clicked position or moves the cursor to
    /// the clicked position.
    fn on_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.is_selecting = true;

        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    /// Deactivates mouse selection.
    fn on_mouse_up(&mut self, _: &MouseUpEvent) {
        self.is_selecting = false;
    }

    /// Extends selection with the mouse
    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    /// Returns the UTF8 offset in which the cursor should be placed after a mouse click (?)
    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }

        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    /// Sets the cursor offset.
    fn move_to(&mut self, offset_utf8: usize, cx: &mut Context<Self>) {
        self.selected_range_utf8 = offset_utf8..offset_utf8;
        cx.notify();
    }

    /// Moves a selection range bound to the offset.
    /// It moves the start bound if selection is reversed, and it moves the end bound otherwise.
    fn select_to(&mut self, offset_utf8: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range_utf8.start = offset_utf8;
        } else {
            self.selected_range_utf8.end = offset_utf8;
        }
        // Reverse selection if range bounds cross
        if self.selected_range_utf8.end < self.selected_range_utf8.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range_utf8 = self.selected_range_utf8.end..self.selected_range_utf8.start;
        }
        cx.notify();
    }

    /// Returns the offset at which the cursor must be drawn
    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range_utf8.start
        } else {
            self.selected_range_utf8.end
        }
    }

    /// Returns the code unit offset of the previous grapheme boundary from the offset passed into it.
    fn previous_boundary(&self, offset_utf8: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset_utf8).then_some(index))
            .unwrap_or(0)
    }

    /// Returns the code unit offset of the next grapheme boundary from the offset passed into it.
    fn next_boundary(&self, offset_utf8: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset_utf8).then_some(index))
            .unwrap_or(self.content.len())
    }

    /// Returns the required number of utf-8 code points to encode the contents up to the utf-16 offset pased into it.
    fn offset_from_utf16(&self, offset_utf16: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for character in self.content.chars() {
            if utf16_count >= offset_utf16 {
                break;
            }
            utf16_count = character.len_utf16();
            utf8_offset = character.len_utf8();
        }

        utf8_offset
    }

    /// Returns the required number of utf-16 code points to encode the contents up to the utf-8 offset into it.
    fn offset_to_utf16(&self, offset_utf8: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for character in self.content.chars() {
            if utf8_count >= offset_utf8 {
                break;
            }
            utf8_count = character.len_utf8();
            utf16_offset = character.len_utf16();
        }

        utf16_offset
    }

    /// Converts a UTF16 offset range to a UTF8 one.
    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    /// Converts a UTF8 offset range to a UTF16 one.
    fn range_to_utf16(&self, range_utf8: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range_utf8.start)..self.offset_to_utf16(range_utf8.end)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range_utf8 = self.range_from_utf16(&range_utf16);
        adjusted_range.replace(self.range_to_utf16(&range_utf8));
        Some(self.content[range_utf8].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range_utf8),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range_utf8
            .as_ref()
            .map(|range_utf8| self.range_to_utf16(range_utf8))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range_utf8 = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range_utf8 = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range_utf8.clone())
            .unwrap_or(self.selected_range_utf8.clone());
        self.content = (self.content[0..range_utf8.start].to_owned()
            + new_text
            + &self.content[range_utf8.end..])
            .into();
        self.selected_range_utf8 =
            range_utf8.start + new_text.len()..range_utf8.start + new_text.len();
        self.marked_range_utf8.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range_utf8 = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range_utf8.clone())
            .unwrap_or(self.selected_range_utf8.clone());
        self.content = (self.content[0..range_utf8.start].to_owned()
            + new_text
            + &self.content[range_utf8.end..])
            .into();
        if !new_text.is_empty() {
            self.marked_range_utf8 = Some(range_utf8.start..range_utf8.start + new_text.len());
        } else {
            self.marked_range_utf8 = None;
        }
        self.selected_range_utf8 = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range_utf8| {
                new_range_utf8.start + range_utf8.start..new_range_utf8.end + range_utf8.end
            })
            .unwrap_or_else(|| {
                range_utf8.start + new_text.len()..range_utf8.start + new_text.len()
            });
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range_utf8 = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                element_bounds.left() + last_layout.x_for_index(range_utf8.start),
                element_bounds.top(),
            ),
            point(
                element_bounds.left() + last_layout.x_for_index(range_utf8.end),
                element_bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        assert_eq!(last_layout.text, self.content);
        let utf8_index = last_layout.index_for_x(point.x - line_point.x)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        todo!()
    }

    fn prepaint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        todo!()
    }

    fn paint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        todo!()
    }
}
