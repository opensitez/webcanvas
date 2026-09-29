use super::{Color, Font, filters::CssFilters};

/// Browser-provided CSS value parsing at the Canvas API boundary.
/// Drawing and rasterization do not depend on a particular CSS engine.
#[derive(Clone, Copy, Debug)]
pub struct CanvasSyntax {
    pub parse_color: fn(&str) -> Option<Color>,
    pub parse_font: fn(&str) -> Option<Font>,
    pub parse_filter: fn(&str) -> CssFilters,
}

impl Default for CanvasSyntax {
    fn default() -> Self {
        Self {
            parse_color: |_| None,
            parse_font: |_| None,
            parse_filter: super::filters::parse_css_filter,
        }
    }
}
