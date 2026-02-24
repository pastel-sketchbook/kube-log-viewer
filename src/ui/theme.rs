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
    GHOST_DARK,
    PASTEL_WAVES_DARK,
    ZOEGI_DARK,
    HIGH_CONTRAST,
    GRUVBOX_LIGHT,
    SOLARIZED_LIGHT,
    FLEXOKI_LIGHT,
    AYU_LIGHT,
    GHOST_LIGHT,
    PASTEL_WAVES_LIGHT,
    ZOEGI_LIGHT,
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

// -- Flexoki Light (ref: flexoki.json) ---------------------------------------

pub const FLEXOKI_LIGHT: Theme = Theme {
    name: "Flexoki Light",

    fg: Color::Rgb(0x10, 0x0f, 0x0f),     // #100F0F
    muted: Color::Rgb(0x6f, 0x6e, 0x69),  // #6F6E69
    accent: Color::Rgb(0x3a, 0xa9, 0x9f), // #3AA99F (cyan)

    header_border: Color::Rgb(0x20, 0x5e, 0xa6), // #205EA6 (blue)
    context_fg: Color::Rgb(0x20, 0x5e, 0xa6),
    namespace_fg: Color::Rgb(0x87, 0x9a, 0x39), // #879A39 (green)

    border_focused: Color::Rgb(0x3a, 0xa9, 0x9f),
    border_unfocused: Color::Rgb(0xe6, 0xe4, 0xd9), // #E6E4D9

    status_running: Color::Rgb(0x87, 0x9a, 0x39),
    status_pending: Color::Rgb(0xa0, 0x7c, 0x10), // #A07C10
    status_succeeded: Color::Rgb(0x20, 0x5e, 0xa6),
    status_failed: Color::Rgb(0xd1, 0x4d, 0x41), // #D14D41
    status_unknown: Color::Rgb(0x6f, 0x6e, 0x69),

    log_error: Color::Rgb(0xd1, 0x4d, 0x41),
    log_warn: Color::Rgb(0xa0, 0x7c, 0x10),
    log_debug: Color::Rgb(0x6f, 0x6e, 0x69),
    zebra_bg: Color::Rgb(0xf2, 0xf0, 0xe5), // list.even.background

    highlight_bg: Color::Rgb(0xf2, 0xf0, 0xe5), // muted.background

    search_fg: Color::Rgb(0xa0, 0x7c, 0x10),
    search_input_bg: Color::Rgb(0xf2, 0xf0, 0xe5),
    search_match_fg: Color::Rgb(0xff, 0xfc, 0xf0), // #FFFCF0 (background)
    search_match_bg: Color::Rgb(0xa0, 0x7c, 0x10),

    statusbar_bg: Color::Rgb(0xf2, 0xf0, 0xe5),
    statusbar_key: Color::Rgb(0x3a, 0xa9, 0x9f),
    statusbar_label: Color::Rgb(0x6f, 0x6e, 0x69),

    popup_border: Color::Rgb(0x3a, 0xa9, 0x9f),
    popup_fg: Color::Rgb(0x10, 0x0f, 0x0f),
};

// -- Solarized Light (ref: solarized.json) -----------------------------------

pub const SOLARIZED_LIGHT: Theme = Theme {
    name: "Solarized Light",

    fg: Color::Rgb(0x58, 0x6e, 0x75),     // #586E75 (base0)
    muted: Color::Rgb(0x93, 0xa1, 0xa1),  // #93A1A1 (base1)
    accent: Color::Rgb(0x26, 0x8b, 0xd2), // #268BD2 (blue)

    header_border: Color::Rgb(0x26, 0x8b, 0xd2),
    context_fg: Color::Rgb(0x26, 0x8b, 0xd2),
    namespace_fg: Color::Rgb(0x85, 0x99, 0x00), // #859900 (green)

    border_focused: Color::Rgb(0x26, 0x8b, 0xd2),
    border_unfocused: Color::Rgb(0xdc, 0xd4, 0xbc), // #DCD4BC

    status_running: Color::Rgb(0x85, 0x99, 0x00),
    status_pending: Color::Rgb(0xb5, 0x89, 0x00), // #B58900 (yellow)
    status_succeeded: Color::Rgb(0x2a, 0xa1, 0x98), // #2AA198 (cyan)
    status_failed: Color::Rgb(0xdc, 0x32, 0x2f),  // #DC322F (red)
    status_unknown: Color::Rgb(0x93, 0xa1, 0xa1),

    log_error: Color::Rgb(0xdc, 0x32, 0x2f),
    log_warn: Color::Rgb(0xb5, 0x89, 0x00),
    log_debug: Color::Rgb(0x93, 0xa1, 0xa1),
    zebra_bg: Color::Rgb(0xee, 0xe8, 0xd5), // #EEE8D5

    highlight_bg: Color::Rgb(0xee, 0xe8, 0xd5),

    search_fg: Color::Rgb(0xb5, 0x89, 0x00),
    search_input_bg: Color::Rgb(0xee, 0xe8, 0xd5),
    search_match_fg: Color::Rgb(0xfd, 0xf6, 0xe3), // #FDF6E3 (background)
    search_match_bg: Color::Rgb(0xb5, 0x89, 0x00),

    statusbar_bg: Color::Rgb(0xee, 0xe8, 0xd5),
    statusbar_key: Color::Rgb(0x26, 0x8b, 0xd2),
    statusbar_label: Color::Rgb(0x93, 0xa1, 0xa1),

    popup_border: Color::Rgb(0x26, 0x8b, 0xd2),
    popup_fg: Color::Rgb(0x58, 0x6e, 0x75),
};

// -- Ghost in the Shell Dark (ref: ghost-in-the-shell.json) ------------------

pub const GHOST_DARK: Theme = Theme {
    name: "Ghost Dark",

    fg: Color::Rgb(0xb3, 0xe5, 0xfc),     // #b3e5fc
    muted: Color::Rgb(0x6b, 0x8e, 0x9e),  // #6b8e9e
    accent: Color::Rgb(0x00, 0xff, 0x9f), // #00ff9f (cyberpunk green)

    header_border: Color::Rgb(0x26, 0xc6, 0xda), // #26c6da (blue)
    context_fg: Color::Rgb(0x26, 0xc6, 0xda),
    namespace_fg: Color::Rgb(0x00, 0xff, 0x9f), // #00ff9f (green)

    border_focused: Color::Rgb(0x00, 0xff, 0x9f),
    border_unfocused: Color::Rgb(0x1a, 0x23, 0x32), // #1a2332

    status_running: Color::Rgb(0x00, 0xff, 0x9f),
    status_pending: Color::Rgb(0xff, 0xa7, 0x26), // #ffa726
    status_succeeded: Color::Rgb(0x26, 0xc6, 0xda),
    status_failed: Color::Rgb(0xff, 0x00, 0x66), // #ff0066
    status_unknown: Color::Rgb(0x6b, 0x8e, 0x9e),

    log_error: Color::Rgb(0xff, 0x00, 0x66),
    log_warn: Color::Rgb(0xff, 0xa7, 0x26),
    log_debug: Color::Rgb(0x6b, 0x8e, 0x9e),
    zebra_bg: Color::Rgb(0x0d, 0x11, 0x17), // #0d1117

    highlight_bg: Color::Rgb(0x0d, 0x11, 0x17),

    search_fg: Color::Rgb(0xff, 0xa7, 0x26),
    search_input_bg: Color::Rgb(0x0d, 0x11, 0x17),
    search_match_fg: Color::Rgb(0x0a, 0x0e, 0x14), // #0a0e14 (background)
    search_match_bg: Color::Rgb(0xff, 0xa7, 0x26),

    statusbar_bg: Color::Rgb(0x0d, 0x11, 0x17),
    statusbar_key: Color::Rgb(0x00, 0xff, 0x9f),
    statusbar_label: Color::Rgb(0x6b, 0x8e, 0x9e),

    popup_border: Color::Rgb(0x00, 0xff, 0x9f),
    popup_fg: Color::Rgb(0xb3, 0xe5, 0xfc),
};

// -- Ghost in the Shell Light (ref: ghost-in-the-shell.json) -----------------

pub const GHOST_LIGHT: Theme = Theme {
    name: "Ghost Light",

    fg: Color::Rgb(0x0a, 0x19, 0x29),     // #0a1929
    muted: Color::Rgb(0x5a, 0x6f, 0x84),  // #5a6f84
    accent: Color::Rgb(0x00, 0x85, 0x77), // #008577

    header_border: Color::Rgb(0x02, 0x77, 0xbd), // #0277bd (blue)
    context_fg: Color::Rgb(0x02, 0x77, 0xbd),
    namespace_fg: Color::Rgb(0x00, 0xa3, 0x90), // #00a390 (green)

    border_focused: Color::Rgb(0x00, 0x85, 0x77),
    border_unfocused: Color::Rgb(0xd0, 0xd7, 0xde), // #d0d7de

    status_running: Color::Rgb(0x00, 0xa3, 0x90),
    status_pending: Color::Rgb(0xe6, 0x77, 0x00), // #e67700
    status_succeeded: Color::Rgb(0x02, 0x77, 0xbd),
    status_failed: Color::Rgb(0xd3, 0x2f, 0x2f), // #d32f2f
    status_unknown: Color::Rgb(0x5a, 0x6f, 0x84),

    log_error: Color::Rgb(0xd3, 0x2f, 0x2f),
    log_warn: Color::Rgb(0xe6, 0x77, 0x00),
    log_debug: Color::Rgb(0x5a, 0x6f, 0x84),
    zebra_bg: Color::Rgb(0xe6, 0xee, 0xf5), // #e6eef5

    highlight_bg: Color::Rgb(0xe6, 0xee, 0xf5),

    search_fg: Color::Rgb(0xe6, 0x77, 0x00),
    search_input_bg: Color::Rgb(0xe6, 0xee, 0xf5),
    search_match_fg: Color::Rgb(0xf0, 0xf4, 0xf8), // #f0f4f8 (background)
    search_match_bg: Color::Rgb(0xe6, 0x77, 0x00),

    statusbar_bg: Color::Rgb(0xe6, 0xee, 0xf5),
    statusbar_key: Color::Rgb(0x00, 0x85, 0x77),
    statusbar_label: Color::Rgb(0x5a, 0x6f, 0x84),

    popup_border: Color::Rgb(0x00, 0x85, 0x77),
    popup_fg: Color::Rgb(0x0a, 0x19, 0x29),
};

// -- High Contrast (ref: high_contrast.ron) ----------------------------------

pub const HIGH_CONTRAST: Theme = Theme {
    name: "High Contrast",

    fg: Color::White,
    muted: Color::Rgb(0xaa, 0xaa, 0xaa), // #AAAAAA
    accent: Color::Cyan,                 // #00FFFF

    header_border: Color::White,
    context_fg: Color::Cyan,
    namespace_fg: Color::Green, // #00FF00

    border_focused: Color::White,
    border_unfocused: Color::Rgb(0x80, 0x80, 0x80), // #808080

    status_running: Color::Green,
    status_pending: Color::Yellow, // #FFFF00
    status_succeeded: Color::Cyan,
    status_failed: Color::Red, // #FF0000
    status_unknown: Color::Rgb(0x80, 0x80, 0x80),

    log_error: Color::Red,
    log_warn: Color::Yellow,
    log_debug: Color::Rgb(0xaa, 0xaa, 0xaa),
    zebra_bg: Color::Rgb(0x1a, 0x1a, 0x1a), // slight gray on pure black

    highlight_bg: Color::Rgb(0x33, 0x33, 0x33), // #333333

    search_fg: Color::Yellow,
    search_input_bg: Color::Rgb(0x33, 0x33, 0x33),
    search_match_fg: Color::Black,
    search_match_bg: Color::Yellow,

    statusbar_bg: Color::Rgb(0x33, 0x33, 0x33),
    statusbar_key: Color::Yellow,
    statusbar_label: Color::Rgb(0xaa, 0xaa, 0xaa),

    popup_border: Color::White,
    popup_fg: Color::White,
};

// -- Pastel Waves Dark (ref: pastel-waves.json) ------------------------------

pub const PASTEL_WAVES_DARK: Theme = Theme {
    name: "Pastel Waves Dark",

    fg: Color::Rgb(0xc5, 0xd9, 0xe8),     // #c5d9e8
    muted: Color::Rgb(0x6b, 0x7e, 0x99),  // #6b7e99
    accent: Color::Rgb(0x6b, 0x9b, 0xc0), // #6b9bc0

    header_border: Color::Rgb(0x8f, 0xc8, 0xe0), // #8fc8e0 (blue)
    context_fg: Color::Rgb(0x8f, 0xc8, 0xe0),
    namespace_fg: Color::Rgb(0x7d, 0xd3, 0xc4), // #7dd3c4 (green)

    border_focused: Color::Rgb(0x6b, 0x9b, 0xc0),
    border_unfocused: Color::Rgb(0x2a, 0x3d, 0x52), // #2a3d52

    status_running: Color::Rgb(0x7d, 0xd3, 0xc4),
    status_pending: Color::Rgb(0xff, 0xe5, 0xa5), // #ffe5a5
    status_succeeded: Color::Rgb(0x8f, 0xc8, 0xe0),
    status_failed: Color::Rgb(0xf5, 0xa9, 0xa4), // #f5a9a4
    status_unknown: Color::Rgb(0x6b, 0x7e, 0x99),

    log_error: Color::Rgb(0xf5, 0xa9, 0xa4),
    log_warn: Color::Rgb(0xff, 0xe5, 0xa5),
    log_debug: Color::Rgb(0x6b, 0x7e, 0x99),
    zebra_bg: Color::Rgb(0x15, 0x24, 0x33), // #152433

    highlight_bg: Color::Rgb(0x1a, 0x2a, 0x3a), // muted.background

    search_fg: Color::Rgb(0xff, 0xe5, 0xa5),
    search_input_bg: Color::Rgb(0x1a, 0x2a, 0x3a),
    search_match_fg: Color::Rgb(0x0d, 0x16, 0x20), // #0d1620 (background)
    search_match_bg: Color::Rgb(0xff, 0xe5, 0xa5),

    statusbar_bg: Color::Rgb(0x1a, 0x2a, 0x3a),
    statusbar_key: Color::Rgb(0x6b, 0x9b, 0xc0),
    statusbar_label: Color::Rgb(0x6b, 0x7e, 0x99),

    popup_border: Color::Rgb(0x6b, 0x9b, 0xc0),
    popup_fg: Color::Rgb(0xc5, 0xd9, 0xe8),
};

// -- Pastel Waves Light (ref: pastel-waves.json) -----------------------------

pub const PASTEL_WAVES_LIGHT: Theme = Theme {
    name: "Pastel Waves Light",

    fg: Color::Rgb(0x2c, 0x5e, 0x79),     // #2c5e79
    muted: Color::Rgb(0x8b, 0xa5, 0xb8),  // #8ba5b8
    accent: Color::Rgb(0x6b, 0x9b, 0xc0), // #6b9bc0

    header_border: Color::Rgb(0x6b, 0x9b, 0xc0),
    context_fg: Color::Rgb(0x6b, 0x9b, 0xc0),
    namespace_fg: Color::Rgb(0x4d, 0xb6, 0xac), // #4db6ac (green)

    border_focused: Color::Rgb(0x6b, 0x9b, 0xc0),
    border_unfocused: Color::Rgb(0xb0, 0xbe, 0xc5), // #b0bec5

    status_running: Color::Rgb(0x4d, 0xb6, 0xac),
    status_pending: Color::Rgb(0xff, 0xd5, 0x4f), // #ffd54f
    status_succeeded: Color::Rgb(0x6b, 0x9b, 0xc0),
    status_failed: Color::Rgb(0xe5, 0x73, 0x73), // #e57373
    status_unknown: Color::Rgb(0x8b, 0xa5, 0xb8),

    log_error: Color::Rgb(0xe5, 0x73, 0x73),
    log_warn: Color::Rgb(0xff, 0xd5, 0x4f),
    log_debug: Color::Rgb(0x8b, 0xa5, 0xb8),
    zebra_bg: Color::Rgb(0xe8, 0xef, 0xf2), // #e8eff2

    highlight_bg: Color::Rgb(0xe8, 0xef, 0xf2),

    search_fg: Color::Rgb(0xff, 0xd5, 0x4f),
    search_input_bg: Color::Rgb(0xe8, 0xef, 0xf2),
    search_match_fg: Color::Rgb(0xf5, 0xf6, 0xf1), // #f5f6f1 (background)
    search_match_bg: Color::Rgb(0xff, 0xd5, 0x4f),

    statusbar_bg: Color::Rgb(0xe8, 0xef, 0xf2),
    statusbar_key: Color::Rgb(0x6b, 0x9b, 0xc0),
    statusbar_label: Color::Rgb(0x8b, 0xa5, 0xb8),

    popup_border: Color::Rgb(0x6b, 0x9b, 0xc0),
    popup_fg: Color::Rgb(0x2c, 0x5e, 0x79),
};

// -- Zoegi Dark (ref: zoegi.json) --------------------------------------------

pub const ZOEGI_DARK: Theme = Theme {
    name: "Zoegi Dark",

    fg: Color::Rgb(0xdd, 0xdd, 0xdd),     // #dddddd
    muted: Color::Rgb(0x99, 0x99, 0x99),  // #999999
    accent: Color::Rgb(0x66, 0xb3, 0x95), // #66b395

    header_border: Color::Rgb(0x72, 0x98, 0xcc), // #7298cc (blue)
    context_fg: Color::Rgb(0x77, 0xb9, 0xc0),    // #77b9c0 (cyan)
    namespace_fg: Color::Rgb(0x66, 0xb3, 0x95),  // #66b395 (green)

    border_focused: Color::Rgb(0x66, 0xb3, 0x95),
    border_unfocused: Color::Rgb(0x33, 0x33, 0x33), // #333333

    status_running: Color::Rgb(0x66, 0xb3, 0x95),
    status_pending: Color::Rgb(0xe7, 0xd3, 0x8f), // #e7d38f
    status_succeeded: Color::Rgb(0x72, 0x98, 0xcc),
    status_failed: Color::Rgb(0xd0, 0x74, 0x68), // #d07468
    status_unknown: Color::Rgb(0x99, 0x99, 0x99),

    log_error: Color::Rgb(0xd0, 0x74, 0x68),
    log_warn: Color::Rgb(0xe7, 0xd3, 0x8f),
    log_debug: Color::Rgb(0x99, 0x99, 0x99),
    zebra_bg: Color::Rgb(0x26, 0x26, 0x26), // #262626

    highlight_bg: Color::Rgb(0x26, 0x26, 0x26),

    search_fg: Color::Rgb(0xe7, 0xd3, 0x8f),
    search_input_bg: Color::Rgb(0x26, 0x26, 0x26),
    search_match_fg: Color::Rgb(0x2b, 0x2b, 0x2b), // #2b2b2b (background)
    search_match_bg: Color::Rgb(0xe7, 0xd3, 0x8f),

    statusbar_bg: Color::Rgb(0x26, 0x26, 0x26),
    statusbar_key: Color::Rgb(0x66, 0xb3, 0x95),
    statusbar_label: Color::Rgb(0x99, 0x99, 0x99),

    popup_border: Color::Rgb(0x66, 0xb3, 0x95),
    popup_fg: Color::Rgb(0xdd, 0xdd, 0xdd),
};

// -- Zoegi Light (ref: zoegi.json) -------------------------------------------

pub const ZOEGI_LIGHT: Theme = Theme {
    name: "Zoegi Light",

    fg: Color::Rgb(0x33, 0x33, 0x33),     // #333333
    muted: Color::Rgb(0x59, 0x59, 0x59),  // #595959
    accent: Color::Rgb(0x37, 0x79, 0x61), // #377961

    header_border: Color::Rgb(0x3e, 0x65, 0x9a), // #3e659a (blue)
    context_fg: Color::Rgb(0x56, 0x8b, 0x99),    // #568b99 (cyan)
    namespace_fg: Color::Rgb(0x37, 0x79, 0x61),  // #377961 (green)

    border_focused: Color::Rgb(0x37, 0x79, 0x61),
    border_unfocused: Color::Rgb(0xe0, 0xe0, 0xe0), // ~#0000001a on white

    status_running: Color::Rgb(0x37, 0x79, 0x61),
    status_pending: Color::Rgb(0xbf, 0x93, 0x40), // #bf9340
    status_succeeded: Color::Rgb(0x56, 0x8b, 0x99),
    status_failed: Color::Rgb(0xcc, 0x5c, 0x5c), // #cc5c5c
    status_unknown: Color::Rgb(0x59, 0x59, 0x59),

    log_error: Color::Rgb(0xcc, 0x5c, 0x5c),
    log_warn: Color::Rgb(0xbf, 0x93, 0x40),
    log_debug: Color::Rgb(0x59, 0x59, 0x59),
    zebra_bg: Color::Rgb(0xf3, 0xf3, 0xf3), // secondary.background

    highlight_bg: Color::Rgb(0xeb, 0xeb, 0xeb), // list.active.background

    search_fg: Color::Rgb(0xbf, 0x93, 0x40),
    search_input_bg: Color::Rgb(0xf3, 0xf3, 0xf3),
    search_match_fg: Color::Rgb(0xff, 0xff, 0xff), // #ffffff (background)
    search_match_bg: Color::Rgb(0xbf, 0x93, 0x40),

    statusbar_bg: Color::Rgb(0xf3, 0xf3, 0xf3),
    statusbar_key: Color::Rgb(0x37, 0x79, 0x61),
    statusbar_label: Color::Rgb(0x59, 0x59, 0x59),

    popup_border: Color::Rgb(0x37, 0x79, 0x61),
    popup_fg: Color::Rgb(0x33, 0x33, 0x33),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_count() {
        assert_eq!(THEMES.len(), 16);
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
