use std::borrow::Cow;
use std::mem::replace;

use helix_core::{LineEnding, Position, RopeGraphemes, RopeSlice};
use helix_view::editor;
use helix_view::graphics::Rect;
use helix_view::theme::Style;

use crate::ui::document::text_render::StyledGrapheme;

#[derive(Debug)]
pub struct DocumentCursorConfig {
    pub max_wrap: usize,
    pub max_indent_retain: usize,
}

impl DocumentCursorConfig {
    pub fn new(editor_config: &editor::Config, viewport: &Rect) -> DocumentCursorConfig {
        DocumentCursorConfig {
            // provide a lower limit to ensure wrapping works well for tiny screens (like the picker)
            max_wrap: editor_config
                .soft_wrap
                .max_wrap
                .min(viewport.width as usize / 4),
            max_indent_retain: editor_config
                .soft_wrap
                .max_indent_retain
                .min(viewport.width as usize / 4),
        }
    }
}

// fn str_is_whitespace(text: &str) -> bool {
//     text.chars().next().map_or(false, |c| c.is_whitespace())
// }

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum WordBoundary {
    Space,
    Wrap,
    Newline,
}

#[derive(Debug)]
pub struct DocumentCursor<'a> {
    pub config: DocumentCursorConfig,
    char_pos: usize,
    word_buf: Vec<StyledGrapheme<'a>>,
    word_width: usize,
    graphemes: RopeGraphemes<'a>,
    /// postition within the document
    pos: Position,
    highlight_scope: (usize, Style),
}

impl<'a> DocumentCursor<'a> {
    pub fn new(
        text: RopeSlice<'a>,
        start_line: usize,
        start_char: usize,
        editor_config: &editor::Config,
        viewport: &Rect,
    ) -> DocumentCursor<'a> {
        DocumentCursor {
            char_pos: start_char,
            word_buf: Vec::with_capacity(if editor_config.soft_wrap.enable {
                viewport.width as usize
            } else {
                64
            }),
            word_width: 0,
            graphemes: RopeGraphemes::new(text),
            pos: Position {
                row: start_line,
                col: 0,
            },
            config: DocumentCursorConfig::new(editor_config, viewport),
            highlight_scope: (0, Style::default()),
        }
    }

    // pub fn byte_pos(&self) -> usize {
    //     self.graphemes.byte_pos()
    // }

    // pub fn char_pos(&self) -> usize {
    //     self.char_pos
    // }

    pub fn doc_position(&self) -> Position {
        self.pos
    }

    pub fn set_highlight_scope(
        &mut self,
        scope_char_start: usize,
        scope_char_end: usize,
        style: Style,
    ) {
        debug_assert_eq!(self.char_pos, scope_char_start);
        self.highlight_scope = (scope_char_end, style)
    }

    pub fn get_highlight_scope(&self) -> (usize, Style) {
        self.highlight_scope
    }

    pub fn advance(&mut self, space_left: usize) -> Option<WordBoundary> {
        loop {
            if self.char_pos >= self.highlight_scope.0 {
                debug_assert_eq!(
                    self.char_pos, self.highlight_scope.0,
                    "Highlight scope must be aligned to grapheme boundary"
                );
                return None;
            }
            if let Some(grapheme) = self.graphemes.next() {
                let codepoints = grapheme.len_chars();
                self.pos.col += codepoints;
                self.char_pos += codepoints;
                if let Some(word_end) =
                    self.push_grapheme::<false>(grapheme, self.highlight_scope.1)
                {
                    return Some(word_end);
                }

                // we reached a point where we need to wrap
                // yield to check if a force wrap is necessary
                if self.word_width >= space_left {
                    return Some(WordBoundary::Wrap);
                }
            } else {
                break;
            }
        }
        None
    }

    pub fn word_width(&self) -> usize {
        self.word_width
    }

    pub fn take_word_buf(&mut self) -> impl Iterator<Item = StyledGrapheme<'a>> + '_ {
        self.word_width = 0;
        self.word_buf.drain(..)
    }

    pub fn take_word_buf_until(
        &mut self,
        mut f: impl FnMut(StyledGrapheme<'a>) -> Option<StyledGrapheme<'a>>,
    ) {
        let mut taken_graphemes = 0;
        for grapheme in &mut self.word_buf {
            if let Some(old_val) = f(replace(grapheme, StyledGrapheme::placeholder())) {
                *grapheme = old_val;
                break;
            }
            self.word_width -= grapheme.min_width() as usize;
            taken_graphemes += 1;
        }
        self.word_buf.drain(..taken_graphemes);
    }

    /// inserts and additional grapheme into the current word
    /// should
    pub fn push_grapheme<const VIRTUAL: bool>(
        &mut self,
        grapheme: RopeSlice<'a>,
        style: Style,
    ) -> Option<WordBoundary> {
        let grapheme = Cow::from(grapheme);

        if LineEnding::from_str(&grapheme).is_some() {
            // we reached EOL reset column and advance the row
            // do not push a grapheme for the line end, instead let the caller handle decide that
            debug_assert!(!VIRTUAL, "inline virtual text must not contain newlines");
            self.pos.row += 1;
            self.pos.col = 0;

            return Some(WordBoundary::Newline);
        }

        let grapheme = StyledGrapheme::new(grapheme, style);
        self.word_width += grapheme.min_width() as usize;
        let word_end = if grapheme.is_breaking_space() {
            Some(WordBoundary::Space)
        } else {
            None
        };

        self.word_buf.push(grapheme);
        word_end
    }
}
