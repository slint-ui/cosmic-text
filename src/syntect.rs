#[cfg(not(feature = "std"))]
use alloc::{
    string::String,
    vec::Vec,
};
#[cfg(feature = "std")]
use std::{
    fs,
    io,
    path::Path,
};
use syntect::highlighting::{
    FontStyle,
    Highlighter,
    HighlightState,
    RangedHighlightIterator,
    Theme,
    ThemeSet,
};
use syntect::parsing::{
    ParseState,
    ScopeStack,
    SyntaxReference,
    SyntaxSet,
};

use crate::{
    Action,
    AttrsList,
    Buffer,
    Color,
    Cursor,
    Editor,
    FontSystem,
    Metrics,
    Style,
    Weight,
};

pub struct SyntaxSystem<'a> {
    pub font_system: &'a FontSystem,
    pub syntax_set: SyntaxSet,
    pub theme_set: ThemeSet,
}

impl<'a> SyntaxSystem<'a> {
    /// Create a new [SyntaxSystem]
    pub fn new(font_system: &'a FontSystem) -> Self {
        Self {
            font_system,
            //TODO: store newlines in buffer
            syntax_set: SyntaxSet::load_defaults_nonewlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }
}

/// A wrapper of [Editor] with syntax highlighting provided by [SyntaxSystem]
pub struct SyntaxEditor<'a> {
    //TODO: should this be pub?
    editor: Editor<'a>,
    syntax_system: &'a SyntaxSystem<'a>,
    syntax: &'a SyntaxReference,
    //TODO: should this be pub?
    pub theme: &'a Theme,
    highlighter: Highlighter<'a>,
    syntax_cache: Vec<(ParseState, HighlightState)>,
}

impl<'a> SyntaxEditor<'a> {
    /// Create a new [SyntaxEditor] with the provided [SyntaxSystem], [Metrics], and theme name.
    /// A good default theme name is "base16-eighties.dark".
    /// Returns None will be returned if theme not found
    pub fn new(syntax_system: &'a SyntaxSystem<'a>, metrics: Metrics, theme_name: &str) -> Option<Self> {
        let editor = Editor::new(Buffer::new(syntax_system.font_system, metrics));
        let syntax = syntax_system.syntax_set.find_syntax_plain_text();
        let theme = syntax_system.theme_set.themes.get(theme_name)?;
        let highlighter = Highlighter::new(theme);

        Some(Self {
            editor,
            syntax_system,
            syntax,
            theme,
            highlighter,
            syntax_cache: Vec::new(),
        })
    }

    /// Load text from a file, and also set syntax to the best option
    #[cfg(feature = "std")]
    pub fn load_text<P: AsRef<Path>>(&mut self, path: P, attrs: crate::Attrs<'a>) -> io::Result<()> {
        let path = path.as_ref();

        let text = fs::read_to_string(path)?;
        self.editor.buffer.set_text(&text, attrs);

        //TODO: re-use text
        self.syntax = match self.syntax_system.syntax_set.find_syntax_for_file(path) {
            Ok(Some(some)) => some,
            Ok(None) => {
                log::warn!("no syntax found for {:?}", path);
                self.syntax_system.syntax_set.find_syntax_plain_text()
            }
            Err(err) => {
                log::warn!("failed to determine syntax for {:?}: {:?}", path, err);
                self.syntax_system.syntax_set.find_syntax_plain_text()
            }
        };

        Ok(())
    }

    /// Shape as needed, also doing syntax highlighting
    pub fn shape_as_needed(&mut self) {
        #[cfg(feature = "std")]
        let now = std::time::Instant::now();

        let mut highlighted = 0;
        for line_i in 0..self.editor.buffer.lines.len() {
            let line = &mut self.editor.buffer.lines[line_i];
            if ! line.is_reset() && line_i < self.syntax_cache.len() {
                continue;
            }
            highlighted += 1;

            let (mut parse_state, mut highlight_state) = if line_i > 0 && line_i <= self.syntax_cache.len() {
                self.syntax_cache[line_i - 1].clone()
            } else {
                (
                    ParseState::new(self.syntax),
                    HighlightState::new(&self.highlighter, ScopeStack::new())
                )
            };

            let ops = parse_state.parse_line(line.text(), &self.syntax_system.syntax_set).unwrap();
            let ranges = RangedHighlightIterator::new(
                &mut highlight_state,
                &ops,
                line.text(),
                &self.highlighter,
            );

            let attrs = line.attrs_list().defaults();
            let mut attrs_list = AttrsList::new(attrs);
            for (style, _, range) in ranges {
                attrs_list.add_span(
                    range,
                    attrs
                        .color(Color::rgba(
                            style.foreground.r,
                            style.foreground.g,
                            style.foreground.b,
                            style.foreground.a,
                        ))
                        //TODO: background
                        .style(if style.font_style.contains(FontStyle::ITALIC) {
                            Style::Italic
                        } else {
                            Style::Normal
                        })
                        .weight(if style.font_style.contains(FontStyle::BOLD) {
                            Weight::BOLD
                        } else {
                            Weight::NORMAL
                        })
                        //TODO: underline
                );
            }

            // Update line attributes. This operation only resets if the line changes
            line.set_attrs_list(attrs_list);
            line.set_wrap_simple(true);

            //TODO: efficiently do syntax highlighting without having to shape whole buffer
            line.shape(&self.syntax_system.font_system);

            let cache_item = (parse_state.clone(), highlight_state.clone());
            if line_i < self.syntax_cache.len() {
                if self.syntax_cache[line_i] != cache_item {
                    self.syntax_cache[line_i] = cache_item;
                    if line_i + 1 < self.editor.buffer.lines.len() {
                        self.editor.buffer.lines[line_i + 1].reset();
                    }
                }
            } else {
                self.syntax_cache.push(cache_item);
            }
        }

        if highlighted > 0 {
            self.editor.buffer.redraw = true;
            #[cfg(feature = "std")]
            log::debug!("Syntax highlighted {} lines in {:?}", highlighted, now.elapsed());
        }

        self.editor.shape_as_needed();
    }

    /// Get the internal [Buffer]
    pub fn buffer(&self) -> &Buffer<'a> {
        &self.editor.buffer
    }

    /// Get the internal [Buffer], mutably
    pub fn buffer_mut(&mut self) -> &mut Buffer<'a> {
        &mut self.editor.buffer
    }

    /// Get the current [Cursor] position
    pub fn cursor(&self) -> Cursor {
        self.editor.cursor()
    }

    /// Copy selection
    pub fn copy_selection(&mut self) -> Option<String> {
        self.editor.copy_selection()
    }

    /// Delete selection, adjusting cursor and returning true if there was a selection
    pub fn delete_selection(&mut self) -> bool {
        self.editor.delete_selection()
    }

    /// Perform an [Action] on the editor
    pub fn action(&mut self, action: Action) {
        self.editor.action(action);
    }

    /// Draw the editor
    #[cfg(feature = "swash")]
    pub fn draw<F>(&self, cache: &mut crate::SwashCache, color: Color, f: F)
        where F: FnMut(i32, i32, u32, u32, Color)
    {
        self.editor.draw(cache, color, f);
    }
}