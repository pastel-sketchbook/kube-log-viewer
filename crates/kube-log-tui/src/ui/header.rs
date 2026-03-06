use jiff::Zoned;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Soft pastel palette for dark backgrounds — light, airy tones that pop
/// against dark terminals.  15 colours, one per non-hyphen letter.
const PASTEL_DARK: [Color; 15] = [
    Color::Rgb(255, 154, 162), // soft pink
    Color::Rgb(255, 183, 178), // peach
    Color::Rgb(255, 218, 193), // apricot
    Color::Rgb(255, 236, 179), // cream yellow
    Color::Rgb(236, 243, 191), // pale chartreuse
    Color::Rgb(226, 240, 203), // pale lime
    Color::Rgb(181, 234, 215), // mint
    Color::Rgb(163, 226, 233), // sky
    Color::Rgb(155, 207, 232), // soft blue
    Color::Rgb(165, 197, 237), // cornflower
    Color::Rgb(175, 187, 235), // periwinkle
    Color::Rgb(199, 178, 232), // lavender
    Color::Rgb(224, 177, 225), // orchid
    Color::Rgb(245, 175, 212), // rose
    Color::Rgb(255, 162, 185), // blush
];

/// Deeper, more saturated palette for light backgrounds — rich enough to stay
/// legible on cream / white surfaces.  Same rainbow order as the dark variant.
const PASTEL_LIGHT: [Color; 15] = [
    Color::Rgb(204, 78, 92),  // deep rose
    Color::Rgb(204, 108, 98), // terracotta
    Color::Rgb(191, 130, 88), // warm clay
    Color::Rgb(179, 152, 62), // golden ochre
    Color::Rgb(134, 158, 68), // olive green
    Color::Rgb(82, 152, 105), // forest mint
    Color::Rgb(48, 152, 138), // deep teal
    Color::Rgb(42, 142, 158), // ocean
    Color::Rgb(52, 122, 175), // cerulean
    Color::Rgb(72, 108, 180), // steel blue
    Color::Rgb(96, 96, 180),  // indigo
    Color::Rgb(126, 88, 175), // grape
    Color::Rgb(158, 82, 158), // plum
    Color::Rgb(180, 72, 132), // magenta
    Color::Rgb(198, 68, 112), // berry
];

/// Return true when the theme background is perceptually "light".
/// Uses the relative-luminance formula (BT.709) on the bg colour;
/// named colours and `Reset` are assumed dark.
fn is_light_theme(theme: &crate::ui::theme::Theme) -> bool {
    match theme.bg {
        Color::Rgb(r, g, b) => {
            let lum = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
            lum > 140.0
        }
        _ => false,
    }
}

/// Build the " kube-log-viewer " title with each letter in a different pastel
/// colour.  Picks the dark or light palette based on the theme background.
/// Hyphens are rendered in the theme's muted colour.
fn pastel_title_spans(theme: &crate::ui::theme::Theme) -> Vec<Span<'static>> {
    let palette = if is_light_theme(theme) {
        &PASTEL_LIGHT
    } else {
        &PASTEL_DARK
    };
    let title = " kube-log-viewer ";
    let mut spans = Vec::with_capacity(title.len());
    let mut color_idx = 0;
    for ch in title.chars() {
        if ch == ' ' {
            spans.push(Span::styled(
                " ",
                Style::default().add_modifier(Modifier::BOLD),
            ));
        } else if ch == '-' {
            spans.push(Span::styled(
                "-",
                Style::default()
                    .fg(theme.muted)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                String::from(ch),
                Style::default()
                    .fg(palette[color_idx % palette.len()])
                    .add_modifier(Modifier::BOLD),
            ));
            color_idx += 1;
        }
    }
    spans
}

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();

    let context = match app.current_context.as_str() {
        "" => "loading...",
        ctx => ctx,
    };

    let namespace = &app.current_namespace;

    let left = Line::from(vec![
        Span::styled(" ctx: ", Style::default().fg(theme.muted)),
        Span::styled(
            context,
            Style::default()
                .fg(theme.context_fg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  |  ", Style::default().fg(theme.muted)),
        Span::styled("ns: ", Style::default().fg(theme.muted)),
        Span::styled(
            namespace.as_str(),
            Style::default()
                .fg(theme.namespace_fg)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let block = Block::default()
        .title(Line::from(pastel_title_spans(theme)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.header_border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Left-aligned context & namespace
    frame.render_widget(Paragraph::new(left), inner);

    // Right-aligned version & datetime
    let now = Zoned::now().strftime("%Y-%m-%d %H:%M:%S").to_string();
    let right = Line::from(vec![
        Span::styled(format!("v{VERSION}"), Style::default().fg(theme.muted)),
        Span::styled("  |  ", Style::default().fg(theme.muted)),
        Span::styled(now, Style::default().fg(theme.muted)),
        Span::raw(" "),
    ]);

    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), inner);
}
