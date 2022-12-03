use std::borrow::Cow;
use std::cmp::min;

use helix_core::graphemes::grapheme_width;
use helix_core::str_utils::char_to_byte_idx;
use helix_core::Position;
use helix_view::editor::{WhitespaceConfig, WhitespaceRenderValue};
use helix_view::graphics::Rect;
use helix_view::theme::Style;
use helix_view::{editor, Document, Theme};
use tui::buffer::Buffer as Surface;

#[derive(Debug)]
/// Various constants required for text rendering.
pub struct TextRenderConfig {
    pub text_style: Style,
    pub whitespace_style: Style,
    pub indent_guide_char: String,
    pub indent_guide_style: Style,
    pub newline: String,
    pub nbsp: String,
    pub space: String,
    pub tab: String,
    pub tab_width: u16,
    pub starting_indent: usize,
}

impl TextRenderConfig {
    pub fn new(
        doc: &Document,
        editor_config: &editor::Config,
        theme: &Theme,
        offset: &Position,
    ) -> TextRenderConfig {
        let WhitespaceConfig {
            render: ws_render,
            characters: ws_chars,
        } = &editor_config.whitespace;

        let tab_width = doc.tab_width();
        let tab = if ws_render.tab() == WhitespaceRenderValue::All {
            std::iter::once(ws_chars.tab)
                .chain(std::iter::repeat(ws_chars.tabpad).take(tab_width - 1))
                .collect()
        } else {
            " ".repeat(tab_width)
        };
        let newline = if ws_render.newline() == WhitespaceRenderValue::All {
            ws_chars.newline.into()
        } else {
            " ".to_owned()
        };

        let space = if ws_render.space() == WhitespaceRenderValue::All {
            ws_chars.space.into()
        } else {
            " ".to_owned()
        };
        let nbsp = if ws_render.nbsp() == WhitespaceRenderValue::All {
            ws_chars.nbsp.into()
        } else {
            " ".to_owned()
        };

        let text_style = theme.get("ui.text");

        TextRenderConfig {
            indent_guide_char: editor_config.indent_guides.character.into(),
            newline,
            nbsp,
            space,
            tab_width: tab_width as u16,
            tab,
            whitespace_style: theme.get("ui.virtual.whitespace"),
            starting_indent: (offset.col / tab_width)
                + editor_config.indent_guides.skip_levels as usize,
            indent_guide_style: text_style.patch(
                theme
                    .try_get("ui.virtual.indent-guide")
                    .unwrap_or_else(|| theme.get("ui.virtual.whitespace")),
            ),
            text_style,
        }
    }
}

#[derive(Debug)]
pub enum Grapheme<'a> {
    Space,
    Nbsp,
    Tab,
    Other { raw: Cow<'a, str>, width: u16 },
}

impl<'a> From<Cow<'a, str>> for Grapheme<'a> {
    fn from(raw: Cow<'a, str>) -> Grapheme<'a> {
        match &*raw {
            "\t" => Grapheme::Tab,
            " " => Grapheme::Space,
            "\u{00A0}" => Grapheme::Nbsp,
            _ => Grapheme::Other {
                width: grapheme_width(&*raw) as u16,
                raw,
            },
        }
    }
}

impl<'a> Grapheme<'a> {
    /// Returns the approximate visual width of this grapheme,
    /// This serves as a lower bound for the width for use during soft wrapping.
    /// The actual displayed witdth might be position dependent and larger (primarly tabs)
    pub fn min_width(&self) -> u16 {
        match *self {
            Grapheme::Other { width, .. } => width,
            _ => 1,
        }
    }

    pub fn into_display(self, visual_x: usize, config: &'a TextRenderConfig) -> (u16, Cow<str>) {
        match self {
            Grapheme::Tab => {
                // make sure we display tab as appropriate amount of spaces
                let visual_tab_width =
                    config.tab_width - (visual_x % config.tab_width as usize) as u16;
                let grapheme_tab_width = char_to_byte_idx(&config.tab, visual_tab_width as usize);
                (
                    visual_tab_width,
                    Cow::from(&config.tab[..grapheme_tab_width]),
                )
            }

            Grapheme::Space => (1, Cow::from(&config.space)),
            Grapheme::Nbsp => (1, Cow::from(&config.nbsp)),
            Grapheme::Other { width, raw: str } => (width, str),
        }
    }

    pub fn is_whitespace(&self) -> bool {
        !matches!(&self, Grapheme::Other { .. })
    }

    pub fn is_breaking_space(&self) -> bool {
        !matches!(&self, Grapheme::Other { .. } | Grapheme::Nbsp)
    }
}

/// A preprossed Grapheme that is ready for rendering
#[derive(Debug)]
pub struct StyledGrapheme<'a> {
    pub grapheme: Grapheme<'a>,
    pub style: Style,
}

impl<'a> StyledGrapheme<'a> {
    pub fn placeholder() -> Self {
        StyledGrapheme {
            grapheme: Grapheme::Space,
            style: Style::default(),
        }
    }

    pub fn new(raw: Cow<'a, str>, style: Style) -> StyledGrapheme<'a> {
        StyledGrapheme {
            grapheme: raw.into(),
            style,
        }
    }

    pub fn is_whitespace(&self) -> bool {
        self.grapheme.is_whitespace()
    }
    pub fn is_breaking_space(&self) -> bool {
        self.grapheme.is_breaking_space()
    }

    pub fn style(&self, config: &TextRenderConfig) -> Style {
        if self.is_whitespace() {
            self.style.patch(config.whitespace_style)
        } else {
            self.style
        }
    }

    /// Returns the approximate visual width of this grapheme,
    /// This serves as a lower bound for the width for use during soft wrapping.
    /// The actual displayed witdth might be position dependent and larger (primarly tabs)
    pub fn min_width(&self) -> u16 {
        self.grapheme.min_width()
    }

    pub fn into_display(
        self,
        visual_x: usize,
        config: &'a TextRenderConfig,
    ) -> (u16, Cow<str>, Style) {
        let style = self.style(config);
        let (width, raw) = self.grapheme.into_display(visual_x, config);
        (width, raw, style)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum IndentLevel {
    /// Indentation is disabled for this line because it wrapped for too long
    None,
    /// Indentation level is not yet known for this line because no non-whitespace char has been reached
    /// The previous indentation level is kept so that indentation guides are not interrupted by empty lines
    Unkown,
    /// Identation level is known for this line
    Known(usize),
}

/// A generic render that can draw lines of text to a surfe
#[derive(Debug)]
pub struct TextRender<'a> {
    /// Surface to render to
    surface: &'a mut Surface,
    /// Various constants required for rendering
    pub config: &'a TextRenderConfig,
    viewport: Rect,
    col_offset: usize,
    visual_line: u16,
    visual_x: usize,
    indent_level: IndentLevel,
    prev_indent_level: usize,
}

impl<'a> TextRender<'a> {
    pub fn new(
        surface: &'a mut Surface,
        config: &'a TextRenderConfig,
        col_offset: usize,
        viewport: Rect,
    ) -> TextRender<'a> {
        TextRender {
            surface,
            config,
            viewport,
            visual_line: 0,
            visual_x: 0,
            col_offset,
            indent_level: IndentLevel::Unkown,
            prev_indent_level: 0,
        }
    }

    /// Returns the indentation of the current line.
    /// If no non-whitespace character has ben encountered yet returns `usize::MAX`
    pub fn indent_level(&self) -> IndentLevel {
        self.indent_level
    }

    // pub fn visual_x(&self) -> usize {
    //     self.visual_x
    // }

    /// Returns the line in the viewport (starting at 0) that will be filled next
    pub fn visual_line(&self) -> u16 {
        self.visual_line
    }

    /// Draws a single `grapheme` at `visual_x` with a specified `style`.
    ///
    /// # Note
    ///
    /// This function assumes that `visual_x` is in-bounds for the viewport.
    fn draw_raw_grapheme_at(&mut self, visual_x: usize, grapheme: &str, style: Style) {
        self.surface.set_string(
            self.viewport.x + (visual_x - self.col_offset) as u16,
            self.viewport.y + self.visual_line,
            grapheme,
            style,
        );
    }

    // /// Draws a single `grapheme` at `visual_x` with a specified `style`.
    // ///
    // /// # Note
    // ///
    // /// This function assumes that `visual_x` is in-bounds for the viewport.
    // pub fn draw_grapheme_at(&mut self, visual_x: usize, styled_grapheme: StyledGrapheme) {
    //     let (_, grapheme, style) = styled_grapheme.into_display(visual_x, self.config);
    //     self.draw_raw_grapheme_at(visual_x, &grapheme, style);
    // }

    /// Draws a single `grapheme` at the current render position with a specified `style`.
    pub fn draw_grapheme(&mut self, styled_grapheme: StyledGrapheme) {
        let cut_off_start = self.col_offset.saturating_sub(self.visual_x as usize);
        let is_whitespace = styled_grapheme.is_whitespace();

        let (width, grapheme, style) = styled_grapheme.into_display(self.visual_x, self.config);
        if self.in_bounds(self.visual_x) {
            self.draw_raw_grapheme_at(self.visual_x, &grapheme, style);
        } else if cut_off_start != 0 && cut_off_start < width as usize {
            // partially on screen
            let rect = Rect::new(
                self.viewport.x as u16,
                self.viewport.y + self.visual_line,
                width - cut_off_start as u16,
                1,
            );
            self.surface.set_style(rect, style);
        }

        if !is_whitespace && matches!(self.indent_level, IndentLevel::Unkown { .. }) {
            self.indent_level = IndentLevel::Known(self.visual_x)
        }
        self.visual_x += width as usize;
    }

    /// Returns whether `visual_x` is inside of the viewport
    fn in_bounds(&self, visual_x: usize) -> bool {
        self.col_offset <= (visual_x as usize)
            && (visual_x as usize) < self.viewport.width as usize + self.col_offset
    }

    pub fn space_left(&self) -> usize {
        (self.col_offset + self.viewport.width as usize).saturating_sub(self.visual_x)
    }

    /// Overlay indentation guides ontop of a rendered line
    /// The indentation level is computed in `draw_lines`.
    /// Therefore this function must always be called afterwards.
    pub fn draw_indent_guides(&mut self) {
        let indent_level = match self.indent_level {
            IndentLevel::None => return, // no identation after wrap
            IndentLevel::Unkown => self.prev_indent_level, //line only contains whitespaces
            IndentLevel::Known(ident_level) => ident_level,
        };
        // Don't draw indent guides outside of view
        let end_indent = min(
            indent_level,
            // Add tab_width - 1 to round up, since the first visible
            // indent might be a bit after offset.col
            self.col_offset + self.viewport.width as usize + (self.config.tab_width - 1) as usize,
        ) / self.config.tab_width as usize;

        for i in self.config.starting_indent..end_indent {
            let x = (self.viewport.x as usize + (i * self.config.tab_width as usize)
                - self.col_offset) as u16;
            let y = self.viewport.y + self.visual_line;
            debug_assert!(self.surface.in_bounds(x, y));
            self.surface.set_string(
                x,
                y,
                &self.config.indent_guide_char,
                self.config.indent_guide_style,
            );
        }
    }

    /// Advances this `TextRender` to the next visual line
    /// This function does not check whether the next line is in bounds.
    pub fn advance_to_next_line(&mut self, new_ident: IndentLevel) {
        self.visual_line += 1;

        if let IndentLevel::Known(indent_level) = self.indent_level {
            self.prev_indent_level = indent_level;
        }
        self.visual_x = match new_ident {
            IndentLevel::None | IndentLevel::Unkown { .. } => 0,
            IndentLevel::Known(ident) => ident,
        };
        self.indent_level = new_ident;
    }

    pub fn skip(&mut self, width: usize) {
        self.visual_x += width
    }

    pub fn reached_viewport_end(&mut self) -> bool {
        self.visual_line >= self.viewport.height
    }
}
