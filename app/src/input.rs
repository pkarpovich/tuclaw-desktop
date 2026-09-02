use std::ops::Range;

use gpui::{
    App, AvailableSpace, Bounds, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Font, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent, PaintQuad,
    Pixels, Point, Render, SharedString, Size, Style, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, WrappedLine, actions, div, fill, point, prelude::*, px, size,
};

use crate::theme;

actions!(
    tuclaw_input,
    [Backspace, MoveLeft, MoveRight, Submit, InsertNewline]
);

const KEY_CONTEXT: &str = "TuclawInput";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(KEY_CONTEXT)),
        KeyBinding::new("left", MoveLeft, Some(KEY_CONTEXT)),
        KeyBinding::new("right", MoveRight, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", Submit, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-enter", InsertNewline, Some(KEY_CONTEXT)),
    ]);
}

pub struct Submitted;

enum Shown {
    Placeholder,
    Content,
}

pub struct TextInput {
    text: String,
    cursor_utf16: usize,
    marked_utf16: Option<Range<usize>>,
    focus: FocusHandle,
    placeholder: SharedString,
    selector: SharedString,
    lines: Vec<WrappedLine>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
}

impl EventEmitter<Submitted> for TextInput {}

impl TextInput {
    pub fn new(
        placeholder: impl Into<SharedString>,
        selector: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> TextInput {
        TextInput {
            text: String::new(),
            cursor_utf16: 0,
            marked_utf16: None,
            focus: cx.focus_handle(),
            placeholder: placeholder.into(),
            selector: selector.into(),
            lines: Vec::new(),
            bounds: Bounds::default(),
            line_height: px(0.),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.text.clear();
        self.cursor_utf16 = 0;
        self.marked_utf16 = None;
        cx.notify();
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    fn shown(&self) -> (SharedString, Shown) {
        if self.text.is_empty() {
            return (self.placeholder.clone(), Shown::Placeholder);
        }
        (SharedString::from(self.text.clone()), Shown::Content)
    }

    fn cursor_byte(&self) -> usize {
        self.offset_from_utf16(self.cursor_utf16)
    }

    fn marked_bytes(&self) -> Option<Range<usize>> {
        let marked = self.marked_utf16.as_ref()?;
        Some(self.range_from_utf16(marked))
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut bytes = 0;
        let mut units = 0;
        for character in self.text.chars() {
            if units >= offset {
                break;
            }
            units += character.len_utf16();
            bytes += character.len_utf8();
        }
        bytes
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut bytes = 0;
        let mut units = 0;
        for character in self.text.chars() {
            if bytes >= offset {
                break;
            }
            bytes += character.len_utf8();
            units += character.len_utf16();
        }
        units
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let Range { start, end } = range;
        self.offset_from_utf16(*start)..self.offset_from_utf16(*end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let Range { start, end } = range;
        self.offset_to_utf16(*start)..self.offset_to_utf16(*end)
    }

    fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        let cursor = self.cursor_utf16;
        self.replace_utf16(cursor..cursor, text);
        cx.notify();
    }

    fn replace_utf16(&mut self, range: Range<usize>, text: &str) {
        let bytes = self.range_from_utf16(&range);
        self.text.replace_range(bytes, text);
        self.cursor_utf16 = range.start + utf16_len(text);
        self.marked_utf16 = None;
    }

    fn backspace(&mut self, _: &Backspace, _window: &mut Window, cx: &mut Context<Self>) {
        if self.cursor_utf16 == 0 {
            return;
        }
        let cursor = self.cursor_byte();
        let previous = previous_boundary(&self.text, cursor);
        let start = self.offset_to_utf16(previous);
        self.replace_utf16(start..self.cursor_utf16, "");
        cx.notify();
    }

    fn move_left(&mut self, _: &MoveLeft, _window: &mut Window, cx: &mut Context<Self>) {
        if self.cursor_utf16 == 0 {
            return;
        }
        let cursor = self.cursor_byte();
        self.cursor_utf16 = self.offset_to_utf16(previous_boundary(&self.text, cursor));
        cx.notify();
    }

    fn move_right(&mut self, _: &MoveRight, _window: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor_byte();
        if cursor >= self.text.len() {
            return;
        }
        self.cursor_utf16 = self.offset_to_utf16(next_boundary(&self.text, cursor));
        cx.notify();
    }

    fn submit(&mut self, _: &Submit, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Submitted);
    }

    fn insert_newline(&mut self, _: &InsertNewline, _window: &mut Window, cx: &mut Context<Self>) {
        self.insert("\n", cx);
    }

    fn take_focus(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
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
        let range = self.range_from_utf16(&range_utf16);
        adjusted_range.replace(self.range_to_utf16(&range));
        Some(self.text[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.cursor_utf16..self.cursor_utf16,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_utf16.clone()
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_utf16 = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = replaced_range(range_utf16, &self.marked_utf16, self.cursor_utf16);
        self.replace_utf16(range, text);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        new_selection: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = replaced_range(range_utf16, &self.marked_utf16, self.cursor_utf16);
        let start = range.start;
        self.replace_utf16(range, text);
        let len = utf16_len(text);
        if len > 0 {
            self.marked_utf16 = Some(start..start + len);
        }
        self.cursor_utf16 = match new_selection {
            Some(Range { start: _, end }) => start + end,
            None => start + len,
        };
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let Range { start, end } = self.range_from_utf16(&range_utf16);
        let start = caret_position(&self.lines, start, self.line_height);
        let end = caret_position(&self.lines, end, self.line_height);
        Some(Bounds::from_corners(
            element_bounds.origin + start,
            element_bounds.origin + point(end.x, end.y + self.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let position = self.bounds.localize(&position)?;
        let mut start = 0;
        let mut top = px(0.);
        for line in &self.lines {
            let height = line.size(self.line_height).height;
            if position.y <= top + height {
                let within = point(position.x, position.y - top);
                let index = match line.index_for_position(within, self.line_height) {
                    Ok(index) => index,
                    Err(index) => index,
                };
                return Some(self.offset_to_utf16(start + index));
            }
            top += height;
            start += line.len() + 1;
        }
        Some(self.offset_to_utf16(self.text.len()))
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selector = self.selector.clone();
        div()
            .id(ElementId::from(self.selector.clone()))
            .debug_selector(move || selector.to_string())
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .w_full()
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::insert_newline))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::take_focus))
            .child(TextElement { input: cx.entity() })
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct Prepaint {
    lines: Vec<WrappedLine>,
    caret: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let (text, _shown) = self.input.read(cx).shown();
        let style = window.text_style();
        let font = style.font();
        let color = style.color;
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let layout = window.request_measured_layout(
            Style::default(),
            move |known, available, window, _cx| {
                let width = match known.width {
                    Some(width) => Some(width),
                    None => match available.width {
                        AvailableSpace::Definite(width) => Some(width),
                        AvailableSpace::MinContent => None,
                        AvailableSpace::MaxContent => None,
                    },
                };
                let runs = runs(text.len(), &font, color, None);
                let lines = shape(window, text.clone(), font_size, &runs, width);
                let mut measured = Size {
                    width: px(0.),
                    height: px(0.),
                };
                for line in &lines {
                    let size = line.size(line_height);
                    measured.height += size.height;
                    measured.width = measured.width.max(size.width);
                }
                if measured.height < line_height {
                    measured.height = line_height;
                }
                Size {
                    width: width.unwrap_or(measured.width),
                    height: measured.height,
                }
            },
        );
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let (text, shown) = input.shown();
        let marked = input.marked_bytes();
        let caret = match shown {
            Shown::Placeholder => 0,
            Shown::Content => input.cursor_byte(),
        };
        let style = window.text_style();
        let color = match shown {
            Shown::Placeholder => theme::text_muted(),
            Shown::Content => style.color,
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let runs = runs(text.len(), &style.font(), color, marked);
        let lines = shape(window, text, font_size, &runs, Some(bounds.size.width));
        let position = caret_position(&lines, caret, line_height);
        let caret = fill(
            Bounds::new(bounds.origin + position, size(px(1.5), line_height)),
            theme::accent(),
        );
        Prepaint {
            lines,
            caret: Some(caret),
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let line_height = window.line_height();
        let lines = std::mem::take(&mut prepaint.lines);
        let mut origin = bounds.origin;
        for line in &lines {
            match line.paint(origin, line_height, TextAlign::Left, None, window, cx) {
                Ok(()) => {}
                Err(error) => eprintln!("a line of the input could not be painted: {error}"),
            }
            origin.y += line.size(line_height).height;
        }
        if focus.is_focused(window)
            && let Some(caret) = prepaint.caret.take()
        {
            window.paint_quad(caret);
        }
        self.input.update(cx, |input, _cx| {
            input.lines = lines;
            input.bounds = bounds;
            input.line_height = line_height;
        });
    }
}

fn shape(
    window: &mut Window,
    text: SharedString,
    font_size: Pixels,
    runs: &[TextRun],
    wrap_width: Option<Pixels>,
) -> Vec<WrappedLine> {
    let shaped = window
        .text_system()
        .shape_text(text, font_size, runs, wrap_width, None);
    let mut lines = Vec::new();
    match shaped {
        Ok(shaped) => {
            for line in shaped {
                lines.push(line);
            }
        }
        Err(error) => eprintln!("the input could not be shaped: {error}"),
    }
    lines
}

fn runs(len: usize, font: &Font, color: Hsla, marked: Option<Range<usize>>) -> Vec<TextRun> {
    let run = TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let Some(Range { start, end }) = marked else {
        return vec![run];
    };
    let mut runs = Vec::new();
    if start > 0 {
        runs.push(TextRun {
            len: start,
            ..run.clone()
        });
    }
    if end > start {
        runs.push(TextRun {
            len: end - start,
            underline: Some(UnderlineStyle {
                color: Some(color),
                thickness: px(1.),
                wavy: false,
            }),
            ..run.clone()
        });
    }
    if len > end {
        runs.push(TextRun {
            len: len - end,
            ..run
        });
    }
    runs
}

fn caret_position(lines: &[WrappedLine], offset: usize, line_height: Pixels) -> Point<Pixels> {
    let mut start = 0;
    let mut top = px(0.);
    for line in lines {
        let end = start + line.len();
        if offset <= end {
            let Some(position) = line.position_for_index(offset - start, line_height) else {
                return point(px(0.), top);
            };
            return point(position.x, top + position.y);
        }
        top += line.size(line_height).height;
        start = end + 1;
    }
    point(px(0.), top)
}

fn replaced_range(
    range: Option<Range<usize>>,
    marked: &Option<Range<usize>>,
    cursor: usize,
) -> Range<usize> {
    let Some(range) = range else {
        let Some(marked) = marked else {
            return cursor..cursor;
        };
        return marked.clone();
    };
    range
}

fn utf16_len(text: &str) -> usize {
    let mut units = 0;
    for character in text.chars() {
        units += character.len_utf16();
    }
    units
}

fn previous_boundary(text: &str, offset: usize) -> usize {
    let mut boundary = offset;
    while boundary > 0 {
        boundary -= 1;
        if text.is_char_boundary(boundary) {
            return boundary;
        }
    }
    0
}

fn next_boundary(text: &str, offset: usize) -> usize {
    let mut boundary = offset;
    while boundary < text.len() {
        boundary += 1;
        if text.is_char_boundary(boundary) {
            return boundary;
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::{
        AppContext, Context, Entity, EntityInputHandler, IntoElement, Modifiers, Pixels, Render,
        TestAppContext, VisualTestContext, Window, div, prelude::*, px,
    };

    use super::{Submitted, TextInput, bind_keys};

    struct Harness {
        input: Entity<TextInput>,
        width: Pixels,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(self.width)
                .text_size(px(13.5))
                .line_height(px(19.))
                .child(self.input.clone())
        }
    }

    fn harness(
        cx: &mut TestAppContext,
        width: Pixels,
    ) -> (Entity<TextInput>, &mut VisualTestContext) {
        cx.update(bind_keys);
        let input = cx.new(|cx| TextInput::new("Message", "input-feed", cx));
        let built = input.clone();
        let (_harness, cx) = cx.add_window_view(move |_window, _cx| Harness {
            input: built,
            width,
        });
        (input, cx)
    }

    fn focused(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| input.read(cx).focus_handle().is_focused(window))
    }

    fn focus(input: &Entity<TextInput>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = input.read(cx).focus_handle().clone();
            focus.focus(window, cx);
        });
        cx.run_until_parked();
    }

    fn submissions(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> Rc<Cell<usize>> {
        let count = Rc::new(Cell::new(0));
        let counter = count.clone();
        let subscription = cx.update(|_window, cx| {
            cx.subscribe(input, move |_input, _event: &Submitted, _cx| {
                counter.set(counter.get() + 1);
            })
        });
        subscription.detach();
        count
    }

    fn drawn_lines(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> usize {
        input.read_with(cx, |input, _cx| {
            (input.bounds.size.height / input.line_height).round() as usize
        })
    }

    #[gpui::test]
    fn typing_fills_the_input_and_moves_the_caret(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        cx.simulate_input("hello");
        input.read_with(cx, |input, _cx| {
            assert_eq!(input.text(), "hello");
            assert_eq!(input.cursor_utf16, 5);
        });
    }

    #[gpui::test]
    fn the_caret_counts_utf16_units_not_bytes(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        cx.simulate_input("привет 🐢");
        input.read_with(cx, |input, _cx| {
            assert_eq!(input.text(), "привет 🐢");
            assert_eq!(input.text().len(), 17);
            assert_eq!(input.cursor_utf16, 9);
        });
    }

    #[gpui::test]
    fn backspace_removes_a_whole_emoji(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        cx.simulate_input("a🐢");
        cx.simulate_keystrokes("backspace");
        input.read_with(cx, |input, _cx| {
            assert_eq!(input.text(), "a");
            assert_eq!(input.cursor_utf16, 1);
        });
    }

    #[gpui::test]
    fn moving_left_inserts_before_the_caret(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        cx.simulate_input("ab");
        cx.simulate_keystrokes("left");
        cx.simulate_input("c");
        input.read_with(cx, |input, _cx| {
            assert_eq!(input.text(), "acb");
            assert_eq!(input.cursor_utf16, 2);
        });
    }

    #[gpui::test]
    fn moving_right_steps_over_a_whole_emoji(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        cx.simulate_input("a🐢b");
        cx.simulate_keystrokes("left left");
        input.read_with(cx, |input, _cx| assert_eq!(input.cursor_utf16, 1));
        cx.simulate_keystrokes("right");
        input.read_with(cx, |input, _cx| assert_eq!(input.cursor_utf16, 3));
        cx.simulate_input("c");
        input.read_with(cx, |input, _cx| {
            assert_eq!(input.text(), "a🐢cb");
            assert_eq!(input.cursor_utf16, 4);
        });
    }

    #[gpui::test]
    fn the_caret_stops_at_both_ends_of_the_buffer(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        cx.simulate_input("ab");
        cx.simulate_keystrokes("right right");
        input.read_with(cx, |input, _cx| assert_eq!(input.cursor_utf16, 2));
        cx.simulate_keystrokes("left left left");
        input.read_with(cx, |input, _cx| assert_eq!(input.cursor_utf16, 0));
        cx.simulate_keystrokes("backspace");
        input.read_with(cx, |input, _cx| {
            assert_eq!(input.text(), "ab");
            assert_eq!(input.cursor_utf16, 0);
        });
    }

    #[gpui::test]
    fn shift_enter_breaks_the_line_and_submits_nothing(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        let count = submissions(&input, cx);
        cx.simulate_input("a");
        cx.simulate_keystrokes("shift-enter");
        cx.simulate_input("b");
        input.read_with(cx, |input, _cx| assert_eq!(input.text(), "a\nb"));
        assert_eq!(count.get(), 0);
    }

    #[gpui::test]
    fn enter_submits(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        let count = submissions(&input, cx);
        cx.simulate_input("hi");
        cx.simulate_keystrokes("enter");
        input.read_with(cx, |input, _cx| assert_eq!(input.text(), "hi"));
        assert_eq!(count.get(), 1);
    }

    #[gpui::test]
    fn clearing_empties_the_input(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        cx.simulate_input("hello");
        input.update(cx, |input, cx| input.clear(cx));
        input.read_with(cx, |input, _cx| {
            assert_eq!(input.text(), "");
            assert!(input.is_blank());
            assert_eq!(input.cursor_utf16, 0);
        });
    }

    #[gpui::test]
    fn marking_text_reports_its_utf16_range(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        input.update_in(cx, |input, window, cx| {
            input.replace_and_mark_text_in_range(None, "ぱ", None, window, cx);
            assert_eq!(input.marked_text_range(window, cx), Some(0..1));
        });
        input.read_with(cx, |input, _cx| assert_eq!(input.text(), "ぱ"));
    }

    #[gpui::test]
    fn marking_again_replaces_the_marked_text(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        input.update_in(cx, |input, window, cx| {
            input.replace_and_mark_text_in_range(None, "ぱ", None, window, cx);
            input.replace_and_mark_text_in_range(None, "ぱす", None, window, cx);
            assert_eq!(input.marked_text_range(window, cx), Some(0..2));
        });
        input.read_with(cx, |input, _cx| {
            assert_eq!(input.text(), "ぱす");
            assert_eq!(input.cursor_utf16, 2);
        });
    }

    #[gpui::test]
    fn unmarking_keeps_the_text(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        input.update_in(cx, |input, window, cx| {
            input.replace_and_mark_text_in_range(None, "ぱ", None, window, cx);
            input.unmark_text(window, cx);
            assert_eq!(input.marked_text_range(window, cx), None);
        });
        input.read_with(cx, |input, _cx| assert_eq!(input.text(), "ぱ"));
    }

    #[gpui::test]
    fn replacing_a_marked_range_clears_the_mark(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        input.update_in(cx, |input, window, cx| {
            input.replace_and_mark_text_in_range(None, "ぱ", None, window, cx);
            input.replace_text_in_range(None, "は", window, cx);
            assert_eq!(input.marked_text_range(window, cx), None);
        });
        input.read_with(cx, |input, _cx| {
            assert_eq!(input.text(), "は");
            assert_eq!(input.cursor_utf16, 1);
        });
    }

    #[gpui::test]
    fn replacing_a_given_utf16_range_counts_units_not_bytes(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        cx.simulate_input("привет 🐢");
        input.update_in(cx, |input, window, cx| {
            input.replace_text_in_range(Some(7..9), "мир", window, cx);
        });
        input.read_with(cx, |input, _cx| {
            assert_eq!(input.text(), "привет мир");
            assert_eq!(input.cursor_utf16, 10);
        });
    }

    #[gpui::test]
    fn a_long_line_wraps_and_grows_the_input(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(90.));
        focus(&input, cx);
        assert_eq!(drawn_lines(&input, cx), 1);
        cx.simulate_input("a sentence long enough to need more than one line");
        assert!(drawn_lines(&input, cx) > 1);
    }

    #[gpui::test]
    fn three_line_breaks_make_four_lines(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        focus(&input, cx);
        cx.simulate_keystrokes("shift-enter shift-enter shift-enter");
        assert_eq!(drawn_lines(&input, cx), 4);
    }

    #[gpui::test]
    fn clicking_the_input_focuses_it(cx: &mut TestAppContext) {
        let (input, cx) = harness(cx, px(400.));
        assert!(!focused(&input, cx));
        let bounds = cx.debug_bounds("input-feed").expect("the input is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert!(focused(&input, cx));
    }
}
