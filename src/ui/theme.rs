use ratatui::style::Color;

/// Semantic color palette for the entire TUI.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: &'static str,

    // Base
    pub fg: Color,
    pub muted: Color,
    pub accent: Color,

    // Header
    pub header_border: Color,
    pub context_fg: Color,
    pub namespace_fg: Color,

    // Borders
    pub border_focused: Color,
    pub border_unfocused: Color,

    // Pod status
    pub status_running: Color,
    pub status_pending: Color,
    pub status_succeeded: Color,
    pub status_failed: Color,
    pub status_unknown: Color,

    // Logs
    pub log_error: Color,
    pub log_warn: Color,
    pub log_debug: Color,
    pub zebra_bg: Color,

    // Selection / highlight
    pub highlight_bg: Color,

    // Search
    pub search_fg: Color,
    pub search_input_bg: Color,
    pub search_match_fg: Color,
    pub search_match_bg: Color,

    // Status bar
    pub statusbar_bg: Color,
    pub statusbar_key: Color,
    pub statusbar_label: Color,

    // Popup
    pub popup_border: Color,
    pub popup_fg: Color,
}

pub const THEMES: &[Theme] = &[
    DARK,
    GRUVBOX_DARK,
    SOLARIZED_DARK,
    FLEXOKI_DARK,
    AYU_DARK,
    GRUVBOX_LIGHT,
    AYU_LIGHT,
];

// -- Built-in default -------------------------------------------------------

pub const DARK: Theme = Theme {
    name: "Dark",

    fg: Color::White,
    muted: Color::DarkGray,
    accent: Color::Cyan,

    header_border: Color::Blue,
    context_fg: Color::Cyan,
    namespace_fg: Color::Green,

    border_focused: Color::Cyan,
    border_unfocused: Color::DarkGray,

    status_running: Color::Green,
    status_pending: Color::Yellow,
    status_succeeded: Color::Blue,
    status_failed: Color::Red,
    status_unknown: Color::DarkGray,

    log_error: Color::Red,
    log_warn: Color::Yellow,
    log_debug: Color::DarkGray,
    zebra_bg: Color::Rgb(25, 25, 25),

    highlight_bg: Color::DarkGray,

    search_fg: Color::Yellow,
    search_input_bg: Color::DarkGray,
    search_match_fg: Color::Black,
    search_match_bg: Color::Yellow,

    statusbar_bg: Color::Rgb(30, 30, 30),
    statusbar_key: Color::Cyan,
    statusbar_label: Color::DarkGray,

    popup_border: Color::Cyan,
    popup_fg: Color::White,
};

// -- Gruvbox Dark (ref: gruvbox.json) ----------------------------------------

pub const GRUVBOX_DARK: Theme = Theme {
    name: "Gruvbox Dark",

    fg: Color::Rgb(0xeb, 0xdb, 0xb2),     // #ebdbb2
    muted: Color::Rgb(0x92, 0x83, 0x74),  // #928374
    accent: Color::Rgb(0xd7, 0x99, 0x21), // #d79921

    header_border: Color::Rgb(0x45, 0x85, 0x88), // #458588
    context_fg: Color::Rgb(0x83, 0xa5, 0x98),    // #83a598
    namespace_fg: Color::Rgb(0x98, 0x97, 0x1a),  // #98971a

    border_focused: Color::Rgb(0xd7, 0x99, 0x21),
    border_unfocused: Color::Rgb(0x50, 0x49, 0x45), // #504945

    status_running: Color::Rgb(0x98, 0x97, 0x1a),
    status_pending: Color::Rgb(0xfa, 0xbd, 0x2f), // #fabd2f
    status_succeeded: Color::Rgb(0x83, 0xa5, 0x98),
    status_failed: Color::Rgb(0xf0, 0x65, 0x55), // #f06555
    status_unknown: Color::Rgb(0x92, 0x83, 0x74),

    log_error: Color::Rgb(0xf0, 0x65, 0x55),
    log_warn: Color::Rgb(0xfa, 0xbd, 0x2f),
    log_debug: Color::Rgb(0x92, 0x83, 0x74),
    zebra_bg: Color::Rgb(0x28, 0x28, 0x28), // list.even.background

    highlight_bg: Color::Rgb(0x3c, 0x38, 0x36), // #3c3836

    search_fg: Color::Rgb(0xfa, 0xbd, 0x2f),
    search_input_bg: Color::Rgb(0x3c, 0x38, 0x36),
    search_match_fg: Color::Rgb(0x1d, 0x20, 0x21), // #1d2021
    search_match_bg: Color::Rgb(0xfa, 0xbd, 0x2f),

    statusbar_bg: Color::Rgb(0x28, 0x28, 0x28), // #282828
    statusbar_key: Color::Rgb(0xd7, 0x99, 0x21),
    statusbar_label: Color::Rgb(0x92, 0x83, 0x74),

    popup_border: Color::Rgb(0xd7, 0x99, 0x21),
    popup_fg: Color::Rgb(0xeb, 0xdb, 0xb2),
};

// -- Solarized Dark (ref: solarized.json) ------------------------------------

pub const SOLARIZED_DARK: Theme = Theme {
    name: "Solarized Dark",

    fg: Color::Rgb(0x93, 0xa1, 0xa1),     // base1
    muted: Color::Rgb(0x58, 0x6e, 0x75),  // base01
    accent: Color::Rgb(0x26, 0x8b, 0xd2), // blue

    header_border: Color::Rgb(0x26, 0x8b, 0xd2),
    context_fg: Color::Rgb(0x26, 0x8b, 0xd2),
    namespace_fg: Color::Rgb(0x85, 0x99, 0x00), // green

    border_focused: Color::Rgb(0x26, 0x8b, 0xd2),
    border_unfocused: Color::Rgb(0x58, 0x6e, 0x75),

    status_running: Color::Rgb(0x85, 0x99, 0x00),
    status_pending: Color::Rgb(0xb5, 0x89, 0x00), // yellow
    status_succeeded: Color::Rgb(0x2a, 0xa1, 0x98), // cyan
    status_failed: Color::Rgb(0xdc, 0x32, 0x2f),  // red
    status_unknown: Color::Rgb(0x58, 0x6e, 0x75),

    log_error: Color::Rgb(0xdc, 0x32, 0x2f),
    log_warn: Color::Rgb(0xb5, 0x89, 0x00),
    log_debug: Color::Rgb(0x58, 0x6e, 0x75),
    zebra_bg: Color::Rgb(0x07, 0x36, 0x42), // base02

    highlight_bg: Color::Rgb(0x07, 0x36, 0x42), // base02

    search_fg: Color::Rgb(0xb5, 0x89, 0x00),
    search_input_bg: Color::Rgb(0x07, 0x36, 0x42),
    search_match_fg: Color::Rgb(0x00, 0x2b, 0x36), // base03
    search_match_bg: Color::Rgb(0xb5, 0x89, 0x00),

    statusbar_bg: Color::Rgb(0x07, 0x36, 0x42),
    statusbar_key: Color::Rgb(0x26, 0x8b, 0xd2),
    statusbar_label: Color::Rgb(0x58, 0x6e, 0x75),

    popup_border: Color::Rgb(0x26, 0x8b, 0xd2),
    popup_fg: Color::Rgb(0x93, 0xa1, 0xa1),
};

// -- Flexoki Dark (ref: flexoki.json) ----------------------------------------

pub const FLEXOKI_DARK: Theme = Theme {
    name: "Flexoki Dark",

    fg: Color::Rgb(0xce, 0xcd, 0xc3),     // #CECDC3
    muted: Color::Rgb(0x87, 0x85, 0x80),  // #878580
    accent: Color::Rgb(0x24, 0x83, 0x7b), // cyan / primary

    header_border: Color::Rgb(0x24, 0x83, 0x7b),
    context_fg: Color::Rgb(0x43, 0x85, 0xbe),   // blue.light
    namespace_fg: Color::Rgb(0x87, 0x9a, 0x39), // green.light

    border_focused: Color::Rgb(0x3a, 0xa9, 0x9f), // cyan.light
    border_unfocused: Color::Rgb(0x34, 0x33, 0x31), // #343331

    status_running: Color::Rgb(0x87, 0x9a, 0x39),
    status_pending: Color::Rgb(0xd0, 0xa2, 0x15), // yellow.light
    status_succeeded: Color::Rgb(0x43, 0x85, 0xbe),
    status_failed: Color::Rgb(0xd1, 0x4d, 0x41), // red.light
    status_unknown: Color::Rgb(0x87, 0x85, 0x80),

    log_error: Color::Rgb(0xd1, 0x4d, 0x41),
    log_warn: Color::Rgb(0xd0, 0xa2, 0x15),
    log_debug: Color::Rgb(0x57, 0x56, 0x53), // #575653
    zebra_bg: Color::Rgb(0x1c, 0x1b, 0x1a),  // list.even.background

    highlight_bg: Color::Rgb(0x1c, 0x1b, 0x1a), // #1C1B1A

    search_fg: Color::Rgb(0xd0, 0xa2, 0x15),
    search_input_bg: Color::Rgb(0x1c, 0x1b, 0x1a),
    search_match_fg: Color::Rgb(0x10, 0x0f, 0x0f), // #100F0F
    search_match_bg: Color::Rgb(0xd0, 0xa2, 0x15),

    statusbar_bg: Color::Rgb(0x1c, 0x1b, 0x1a),
    statusbar_key: Color::Rgb(0x3a, 0xa9, 0x9f),
    statusbar_label: Color::Rgb(0x87, 0x85, 0x80),

    popup_border: Color::Rgb(0x3a, 0xa9, 0x9f),
    popup_fg: Color::Rgb(0xce, 0xcd, 0xc3),
};

// -- Ayu Dark (ref: ayu.json) ------------------------------------------------

pub const AYU_DARK: Theme = Theme {
    name: "Ayu Dark",

    fg: Color::Rgb(0xbf, 0xbd, 0xb6),     // #bfbdb6
    muted: Color::Rgb(0x52, 0x51, 0x4f),  // #52514f
    accent: Color::Rgb(0x5a, 0xc1, 0xfe), // #5ac1fe

    header_border: Color::Rgb(0x36, 0xa3, 0xd9), // #36A3D9
    context_fg: Color::Rgb(0x5a, 0xc1, 0xfe),
    namespace_fg: Color::Rgb(0xaa, 0xd8, 0x4c), // #aad84c

    border_focused: Color::Rgb(0x5a, 0xc1, 0xfe),
    border_unfocused: Color::Rgb(0x4b, 0x4c, 0x4e), // #4b4c4e

    status_running: Color::Rgb(0xaa, 0xd8, 0x4c),
    status_pending: Color::Rgb(0xfe, 0xb4, 0x54), // #feb454
    status_succeeded: Color::Rgb(0x5a, 0xc1, 0xfe),
    status_failed: Color::Rgb(0xef, 0x71, 0x77), // #ef7177
    status_unknown: Color::Rgb(0x52, 0x51, 0x4f),

    log_error: Color::Rgb(0xef, 0x71, 0x77),
    log_warn: Color::Rgb(0xfe, 0xb4, 0x54),
    log_debug: Color::Rgb(0x52, 0x51, 0x4f),
    zebra_bg: Color::Rgb(0x19, 0x1f, 0x2a), // list.even.background

    highlight_bg: Color::Rgb(0x1f, 0x21, 0x27), // #1f2127

    search_fg: Color::Rgb(0xfe, 0xb4, 0x54),
    search_input_bg: Color::Rgb(0x1f, 0x21, 0x27),
    search_match_fg: Color::Rgb(0x0d, 0x10, 0x16), // #0D1016
    search_match_bg: Color::Rgb(0xfe, 0xb4, 0x54),

    statusbar_bg: Color::Rgb(0x1f, 0x21, 0x27),
    statusbar_key: Color::Rgb(0x5a, 0xc1, 0xfe),
    statusbar_label: Color::Rgb(0x52, 0x51, 0x4f),

    popup_border: Color::Rgb(0x5a, 0xc1, 0xfe),
    popup_fg: Color::Rgb(0xbf, 0xbd, 0xb6),
};

// -- Gruvbox Light (ref: gruvbox.json) ---------------------------------------

pub const GRUVBOX_LIGHT: Theme = Theme {
    name: "Gruvbox Light",

    fg: Color::Rgb(0x3c, 0x38, 0x36),     // #3c3836
    muted: Color::Rgb(0x92, 0x83, 0x74),  // #928374
    accent: Color::Rgb(0xd7, 0x99, 0x21), // #d79921

    header_border: Color::Rgb(0x07, 0x66, 0x78), // #076678
    context_fg: Color::Rgb(0x07, 0x66, 0x78),
    namespace_fg: Color::Rgb(0x67, 0xa6, 0x4f), // #67a64f

    border_focused: Color::Rgb(0xd7, 0x99, 0x21),
    border_unfocused: Color::Rgb(0xd5, 0xc4, 0xa1), // #d5c4a1

    status_running: Color::Rgb(0x67, 0xa6, 0x4f),
    status_pending: Color::Rgb(0xb5, 0x76, 0x14), // #b57614
    status_succeeded: Color::Rgb(0x45, 0x85, 0x88),
    status_failed: Color::Rgb(0xcc, 0x24, 0x1d), // #cc241d
    status_unknown: Color::Rgb(0x92, 0x83, 0x74),

    log_error: Color::Rgb(0xcc, 0x24, 0x1d),
    log_warn: Color::Rgb(0xb5, 0x76, 0x14),
    log_debug: Color::Rgb(0x92, 0x83, 0x74),
    zebra_bg: Color::Rgb(0xf9, 0xec, 0xba), // list.even.background

    highlight_bg: Color::Rgb(0xeb, 0xdb, 0xb2), // #ebdbb2

    search_fg: Color::Rgb(0xb5, 0x76, 0x14),
    search_input_bg: Color::Rgb(0xeb, 0xdb, 0xb2),
    search_match_fg: Color::Rgb(0xfb, 0xf1, 0xc7), // #fbf1c7
    search_match_bg: Color::Rgb(0xb5, 0x76, 0x14),

    statusbar_bg: Color::Rgb(0xeb, 0xdb, 0xb2),
    statusbar_key: Color::Rgb(0x07, 0x66, 0x78),
    statusbar_label: Color::Rgb(0x92, 0x83, 0x74),

    popup_border: Color::Rgb(0xd7, 0x99, 0x21),
    popup_fg: Color::Rgb(0x3c, 0x38, 0x36),
};

// -- Ayu Light (ref: ayu.json) -----------------------------------------------

pub const AYU_LIGHT: Theme = Theme {
    name: "Ayu Light",

    fg: Color::Rgb(0x5c, 0x61, 0x66),     // #5c6166
    muted: Color::Rgb(0x99, 0xa0, 0xa6),  // #99a0a6
    accent: Color::Rgb(0x55, 0xb4, 0xd3), // #55b4d3

    header_border: Color::Rgb(0x55, 0xb4, 0xd3),
    context_fg: Color::Rgb(0x55, 0xb4, 0xd3),
    namespace_fg: Color::Rgb(0x85, 0xb3, 0x04), // #85b304

    border_focused: Color::Rgb(0x55, 0xb4, 0xd3),
    border_unfocused: Color::Rgb(0xcf, 0xd1, 0xd2), // #cfd1d2

    status_running: Color::Rgb(0x85, 0xb3, 0x04),
    status_pending: Color::Rgb(0xf1, 0xad, 0x49), // #f1ad49
    status_succeeded: Color::Rgb(0x55, 0xb4, 0xd3),
    status_failed: Color::Rgb(0xf0, 0x71, 0x71), // #F07171
    status_unknown: Color::Rgb(0x99, 0xa0, 0xa6),

    log_error: Color::Rgb(0xf0, 0x71, 0x71),
    log_warn: Color::Rgb(0xf1, 0xad, 0x49),
    log_debug: Color::Rgb(0x99, 0xa0, 0xa6),
    zebra_bg: Color::Rgb(0xe6, 0xe6, 0xe6), // list.even.background

    highlight_bg: Color::Rgb(0xec, 0xec, 0xed), // #ECECED

    search_fg: Color::Rgb(0xf1, 0xad, 0x49),
    search_input_bg: Color::Rgb(0xec, 0xec, 0xed),
    search_match_fg: Color::Rgb(0xfc, 0xfc, 0xfc), // #FCFCFC
    search_match_bg: Color::Rgb(0xf1, 0xad, 0x49),

    statusbar_bg: Color::Rgb(0xec, 0xec, 0xed),
    statusbar_key: Color::Rgb(0x55, 0xb4, 0xd3),
    statusbar_label: Color::Rgb(0x99, 0xa0, 0xa6),

    popup_border: Color::Rgb(0x55, 0xb4, 0xd3),
    popup_fg: Color::Rgb(0x5c, 0x61, 0x66),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_count() {
        assert_eq!(THEMES.len(), 7);
    }

    #[test]
    fn test_cycle_wraps() {
        for i in 0..THEMES.len() {
            let next = (i + 1) % THEMES.len();
            assert!(next < THEMES.len());
        }
    }

    #[test]
    fn test_theme_names_unique() {
        let names: Vec<&str> = THEMES.iter().map(|t| t.name).collect();
        for (i, name) in names.iter().enumerate() {
            assert!(!name.is_empty());
            assert!(
                !names[i + 1..].contains(name),
                "duplicate theme name: {name}"
            );
        }
    }
}
