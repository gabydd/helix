use helix_core::syntax::Highlight;
use helix_core::RopeSlice;
use helix_core::{syntax::HighlightEvent, Position};
use helix_view::editor;
use helix_view::{graphics::Rect, Theme};

use crate::ui::document::cursor::WordBoundary;
use crate::ui::document::text_render::{Grapheme, IndentLevel, StyledGrapheme};
pub use cursor::DocumentCursor;
pub use text_render::{TextRender, TextRenderConfig};

mod cursor;
mod text_render;

pub struct DocumentRender<'a, H: Iterator<Item = HighlightEvent>> {
    pub config: &'a editor::Config,
    pub theme: &'a Theme,

    pub text: RopeSlice<'a>,
    highlights: H,

    pub cursor: DocumentCursor<'a>,
    spans: Vec<Highlight>,

    finished: bool,
}

impl<'a, H: Iterator<Item = HighlightEvent>> DocumentRender<'a, H> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: &'a editor::Config,
        theme: &'a Theme,
        text: RopeSlice<'a>,
        highlights: H,
        viewport: Rect,
        offset: Position,
        char_offset: usize,
        text_render: &mut TextRender,
    ) -> Self {
        let mut render = DocumentRender {
            config,
            theme,
            text,
            highlights,
            cursor: DocumentCursor::new(text, offset.row, char_offset, config, &viewport),
            // render: TextRender::new(surface, render_config, offset.col, viewport),
            spans: Vec::with_capacity(64),
            finished: false,
        };

        // advance to first highlight scope
        render.advance_highlight_scope(text_render);
        render
    }

    /// Returns the line in the doucment that will be rendered next
    pub fn doc_line(&self) -> usize {
        self.cursor.doc_position().row
    }

    fn advance_highlight_scope(&mut self, text_render: &mut TextRender) {
        while let Some(event) = self.highlights.next() {
            match event {
                HighlightEvent::HighlightStart(span) => self.spans.push(span),
                HighlightEvent::HighlightEnd => {
                    self.spans.pop();
                }
                HighlightEvent::Source { start, end } => {
                    if start == end {
                        continue;
                    }
                    // TODO cursor end
                    let style = self
                        .spans
                        .iter()
                        .fold(text_render.config.text_style, |acc, span| {
                            acc.patch(self.theme.highlight(span.0))
                        });
                    self.cursor.set_highlight_scope(start, end, style);
                    return;
                }
            }
        }
        self.finished = true;
    }

    /// Perform a softwrap
    fn wrap_line(&mut self, text_render: &mut TextRender) {
        // Fully wrapping this word would wrap too much wrap
        // wrap inside the word instead
        if self.cursor.word_width() > self.cursor.config.max_wrap {
            self.cursor.take_word_buf_until(|grapheme| {
                if text_render.space_left() < grapheme.min_width() as usize {
                    // leave this grapheme in the word_buf
                    Some(grapheme)
                } else {
                    text_render.draw_grapheme(grapheme);
                    None
                }
            });
        }

        let indent_level = match text_render.indent_level() {
            IndentLevel::Known(indent_level)
                if indent_level <= self.cursor.config.max_indent_retain =>
            {
                IndentLevel::Known(indent_level)
            }
            _ => IndentLevel::None,
        };
        self.advance_line(indent_level, text_render);
        text_render.skip(self.config.soft_wrap.wrap_indent);
    }

    /// Returns whether this document renderer finished rendering
    /// either because the viewport end or EOF was reached
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Renders the next line of the document.
    /// If softwrapping is enabled this may only correspond to rendering a part of the line
    ///
    /// # Returns
    ///
    /// Whether the rendered line was only partially rendered because the viewport end was reached
    pub fn render_line(&mut self, text_render: &mut TextRender) {
        if self.finished {
            return;
        }

        loop {
            while let Some(word_boundry) = self.cursor.advance(if self.config.soft_wrap.enable {
                text_render.space_left()
            } else {
                // limit word size to maintain fast performance for large lines
                64
            }) {
                if self.config.soft_wrap.enable && word_boundry == WordBoundary::Wrap {
                    self.wrap_line(text_render);
                    return;
                }

                self.render_word(text_render);

                if word_boundry == WordBoundary::Newline {
                    // render EOL space
                    text_render.draw_grapheme(StyledGrapheme {
                        grapheme: Grapheme::Space,
                        style: self.cursor.get_highlight_scope().1,
                    });
                    self.advance_line(IndentLevel::Unkown, text_render);
                    self.finished = text_render.reached_viewport_end();
                    return;
                }

                if text_render.reached_viewport_end() {
                    self.finished = true;
                    return;
                }
            }

            self.advance_highlight_scope(text_render);

            // we properly reached the text end, this is the end of the last line
            // render remaining text
            if self.finished {
                self.render_word(text_render);

                if self.cursor.get_highlight_scope().0 > self.text.len_chars() {
                    // trailing cursor is rendered as a whitespace
                    text_render.draw_grapheme(StyledGrapheme {
                        grapheme: Grapheme::Space,
                        style: self.cursor.get_highlight_scope().1,
                    });
                }

                self.finish_line(text_render);
                return;
            }

            // we reached the viewport end but the line was only partially rendered
            if text_render.reached_viewport_end() {
                self.finish_line(text_render);
                self.finished = true;
                return;
            }
        }
    }

    fn render_word(&mut self, text_render: &mut TextRender) {
        for grapheme in self.cursor.take_word_buf() {
            text_render.draw_grapheme(grapheme);
        }
    }

    fn finish_line(&mut self, text_render: &mut TextRender) {
        if self.config.indent_guides.render {
            text_render.draw_indent_guides()
        }
    }

    fn advance_line(&mut self, next_indent_level: IndentLevel, text_render: &mut TextRender) {
        self.finish_line(text_render);
        text_render.advance_to_next_line(next_indent_level);
    }
}
