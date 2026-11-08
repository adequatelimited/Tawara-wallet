// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The design tokens (colours, spacing, radii) and the widget styles built
//! from them. The design is dark only, so this is the only theme.

use iced::border::{self, Border, Radius};
use iced::widget::{container, rule, scrollable, text_input};
use iced::{Background, Color, Shadow, Theme};

/// Colours. All opaque.
pub mod color {
    use iced::Color;

    // Backgrounds and surfaces.
    /// Page background; also the inset fill of fields.
    pub const BG_APP: Color = Color::from_rgb8(0x1D, 0x2A, 0x2E);
    /// Sidebar, cards, radio cards, search fields, stat tiles.
    pub const BG_SURFACE: Color = Color::from_rgb8(0x17, 0x21, 0x24);
    /// Neutral chips, tonal buttons, outgoing-row icon circles.
    pub const BG_RAISED: Color = Color::from_rgb8(0x24, 0x35, 0x3A);
    /// Active nav, settled chips, green callouts, selected rows.
    pub const ACCENT_SOFT: Color = Color::from_rgb8(0x17, 0x3B, 0x33);
    /// Settling chips, pending icon circles, warning tonal buttons.
    pub const WARNING_SOFT: Color = Color::from_rgb8(0x3D, 0x35, 0x20);
    /// Amber callouts and warning table rows.
    pub const WARNING_SURFACE: Color = Color::from_rgb8(0x2A, 0x2A, 0x1F);

    // Borders and dividers.
    pub const BORDER_SUBTLE: Color = Color::from_rgb8(0x2A, 0x3A, 0x3F);
    pub const BORDER_CONTROL: Color = Color::from_rgb8(0x2E, 0x40, 0x45);
    pub const BORDER_STRONG: Color = Color::from_rgb8(0x3A, 0x4D, 0x52);
    pub const ACCENT_BORDER: Color = Color::from_rgb8(0x1F, 0x5A, 0x49);
    pub const WARNING_BORDER: Color = Color::from_rgb8(0x5A, 0x4A, 0x22);
    /// Only the "Normal" legend swatch under the network card's newest
    /// blocks.
    pub const SWATCH_NORMAL: Color = Color::from_rgb8(0x4A, 0x5E, 0x63);

    // Text.
    pub const TEXT_PRIMARY: Color = Color::from_rgb8(0xFF, 0xFF, 0xFF);
    pub const TEXT_SECONDARY: Color = Color::from_rgb8(0xB4, 0xC2, 0xC5);
    pub const TEXT_MUTED: Color = Color::from_rgb8(0x8F, 0xA0, 0xA4);
    pub const TEXT_ON_ACCENT_SOFT: Color = Color::from_rgb8(0xCF, 0xEF, 0xE2);
    pub const TEXT_ON_WARNING_SOFT: Color = Color::from_rgb8(0xF2, 0xE3, 0xBF);
    /// Text and icons on accent and warning fills; the same value as
    /// [`BG_APP`].
    pub const TEXT_ON_ACCENT: Color = Color::from_rgb8(0x1D, 0x2A, 0x2E);
    /// Placeholder text in fields.
    pub const PLACEHOLDER: Color = Color::from_rgb8(0x75, 0x75, 0x75);

    // Accent and warning. No screen uses an error colour.
    pub const ACCENT: Color = Color::from_rgb8(0x15, 0xDC, 0x96);
    pub const ACCENT_HOVER: Color = Color::from_rgb8(0x5F, 0xF0, 0xBC);
    pub const WARNING: Color = Color::from_rgb8(0xF5, 0xB8, 0x41);
}

/// Spacing in pixels.
pub mod space {
    pub const S2: f32 = 2.0;
    pub const S4: f32 = 4.0;
    pub const S6: f32 = 6.0;
    pub const S8: f32 = 8.0;
    pub const S10: f32 = 10.0;
    pub const S12: f32 = 12.0;
    pub const S14: f32 = 14.0;
    pub const S16: f32 = 16.0;
    pub const S18: f32 = 18.0;
    pub const S20: f32 = 20.0;
    pub const S24: f32 = 24.0;
    pub const S28: f32 = 28.0;
    pub const S32: f32 = 32.0;
    pub const S40: f32 = 40.0;
    pub const S48: f32 = 48.0;
    pub const S56: f32 = 56.0;
}

/// Corner radii in pixels.
pub mod radius {
    pub const R2: f32 = 2.0;
    pub const R8: f32 = 8.0;
    pub const R10: f32 = 10.0;
    pub const R12: f32 = 12.0;
    pub const R14: f32 = 14.0;
    pub const R16: f32 = 16.0;
    pub const R20: f32 = 20.0;
    pub const R24: f32 = 24.0;
}

/// The application's theme: iced's own, with the design's page and text
/// colours, so widgets left at their defaults match the page.
#[must_use]
pub fn theme() -> Theme {
    Theme::custom(
        "Tawara".to_owned(),
        iced::theme::Palette {
            background: color::BG_APP,
            text: color::TEXT_PRIMARY,
            primary: color::ACCENT,
            success: color::ACCENT,
            warning: color::WARNING,
            // No screen uses an error colour; iced requires one, so it is the
            // warning amber and nothing red appears.
            danger: color::WARNING,
        },
    )
}

fn border(width: f32, color: Color, radius: f32) -> Border {
    Border {
        color,
        width,
        radius: Radius::from(radius),
    }
}

// ---------------------------------------------------------------- containers

/// The page itself.
pub fn page(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(color::BG_APP)),
        text_color: Some(color::TEXT_PRIMARY),
        ..container::Style::default()
    }
}

/// The sidebar: the surface colour. The caller draws its right-edge divider
/// as a rule.
pub fn sidebar(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(color::BG_SURFACE)),
        text_color: Some(color::TEXT_PRIMARY),
        ..container::Style::default()
    }
}

/// A standard card: surface fill, subtle border, radius 20.
pub fn card(_: &Theme) -> container::Style {
    surface(color::BG_SURFACE, color::BORDER_SUBTLE, radius::R20)
}

/// A hero card: as a card, radius 24.
pub fn hero_card(_: &Theme) -> container::Style {
    surface(color::BG_SURFACE, color::BORDER_SUBTLE, radius::R24)
}

/// A stat tile: surface fill, subtle border, radius 16.
pub fn stat_tile(_: &Theme) -> container::Style {
    surface(color::BG_SURFACE, color::BORDER_SUBTLE, radius::R16)
}

/// The highlighted stat tile.
pub fn stat_tile_accent(_: &Theme) -> container::Style {
    surface(color::ACCENT_SOFT, color::ACCENT_BORDER, radius::R16)
}

/// An inset panel, such as a node tile: the page colour inside a card,
/// radius 12, no border.
pub fn inset(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(color::BG_APP)),
        border: border(0.0, Color::TRANSPARENT, radius::R12),
        ..container::Style::default()
    }
}

/// The sidebar's network panel.
pub fn sidebar_panel(_: &Theme) -> container::Style {
    surface(color::BG_APP, color::BORDER_SUBTLE, radius::R14)
}

/// A green callout.
pub fn callout_accent(_: &Theme) -> container::Style {
    container::Style {
        text_color: Some(color::TEXT_ON_ACCENT_SOFT),
        ..surface(color::ACCENT_SOFT, color::ACCENT_BORDER, radius::R20)
    }
}

/// An amber callout.
pub fn callout_warning(_: &Theme) -> container::Style {
    container::Style {
        text_color: Some(color::TEXT_ON_WARNING_SOFT),
        ..surface(color::WARNING_SURFACE, color::WARNING_BORDER, radius::R16)
    }
}

/// The square tile behind a callout's icon.
pub fn icon_tile(fill: Color, ink: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(fill)),
        text_color: Some(ink),
        border: border(0.0, Color::TRANSPARENT, radius::R12),
        ..container::Style::default()
    }
}

/// A pill: chips and badges.
pub fn pill(fill: Color, ink: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(fill)),
        text_color: Some(ink),
        border: border(0.0, Color::TRANSPARENT, 999.0),
        ..container::Style::default()
    }
}

/// A selected table row: the soft accent fill with a green ring.
pub fn row_selected(_: &Theme) -> container::Style {
    surface(color::ACCENT_SOFT, color::ACCENT, radius::R12)
}

/// A warning table row.
pub fn row_warning(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(color::WARNING_SURFACE)),
        border: border(0.0, Color::TRANSPARENT, radius::R12),
        ..container::Style::default()
    }
}

/// A radio card, selected or not.
pub fn radio_card(selected: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        surface(
            color::BG_SURFACE,
            if selected {
                color::ACCENT
            } else {
                color::BORDER_SUBTLE
            },
            radius::R16,
        )
    }
}

/// A switch, such as the explorer's auto-update: green with a dark knob when
/// on, raised with a muted knob when off, a strong edge while hovered.
pub fn switch(_: &Theme, status: iced::widget::toggler::Status) -> iced::widget::toggler::Style {
    use iced::widget::toggler::Status;
    let (on, hovered) = match status {
        Status::Active { is_toggled } | Status::Disabled { is_toggled } => (is_toggled, false),
        Status::Hovered { is_toggled } => (is_toggled, true),
    };
    iced::widget::toggler::Style {
        background: Background::Color(if on { color::ACCENT } else { color::BG_RAISED }),
        background_border_width: 1.0,
        background_border_color: if hovered {
            color::BORDER_STRONG
        } else if on {
            color::ACCENT
        } else {
            color::BORDER_SUBTLE
        },
        foreground: Background::Color(if on {
            color::TEXT_ON_ACCENT
        } else {
            color::TEXT_SECONDARY
        }),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: Some(color::TEXT_SECONDARY),
        border_radius: None,
        padding_ratio: 0.1,
    }
}

/// One of the network card's newest blocks: its kind's fill, and its edge
/// (the green ring for the current tip).
pub fn block_tile(fill: Color, edge: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| surface(fill, edge, radius::R10)
}

/// A legend swatch.
pub fn swatch(fill: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| surface(fill, fill, radius::R2)
}

/// A field-like box drawn as a container (the account selector, a read-only
/// value): the page colour with the control border.
pub fn field_box(_: &Theme) -> container::Style {
    surface(color::BG_APP, color::BORDER_CONTROL, radius::R12)
}

/// The brand panel of the first-run screen.
pub fn brand_panel(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(color::ACCENT)),
        text_color: Some(color::TEXT_ON_ACCENT),
        ..container::Style::default()
    }
}

fn surface(fill: Color, edge: Color, r: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(fill)),
        text_color: Some(color::TEXT_PRIMARY),
        border: border(1.0, edge, r),
        shadow: Shadow::default(),
        snap: true,
    }
}

// ------------------------------------------------------------------- buttons

/// The button variants that differ in colour. Sizes and padding are set
/// where each button is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// Green fill, dark text.
    Primary,
    /// Amber fill, dark text.
    WarningPrimary,
    /// Transparent with the strong border (the header "Receive").
    OutlineStrong,
    /// Transparent with the control border.
    Secondary,
    /// The raised fill, no border.
    Tonal,
    /// The soft amber fill and amber text ("Reconcile").
    WarningTonal,
    /// Transparent with the amber border ("Inspect tag").
    WarningOutline,
    /// No fill, no border, muted glyph (remove row).
    Ghost,
    /// A text link: no box, accent text.
    Link,
    /// A nav item, active or not.
    Nav { active: bool },
    /// A segment of a segmented control or a filter tab.
    Segment { selected: bool },
    /// The dashed "add" button.
    Dashed,
    /// No fill, no border, primary text: a button that is a card or a row,
    /// drawing its own box.
    Bare,
}

/// A segment of a segmented control or a filter row, drawn as a container
/// (`ui::segment`): the fill, ink and radius of [`button`]'s
/// `Button::Segment { selected }` in the state `enabled` says, so that the
/// two draw the same pixels.
pub fn segment(selected: bool, enabled: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let fill = selected.then_some(color::ACCENT);
        // A disabled segment that is selected keeps its shape and dims to the
        // raised fill, as `button` dims a filled button.
        let fill = if !enabled && fill.is_some() {
            Some(color::BG_RAISED)
        } else {
            fill
        };
        let ink = if !enabled {
            color::TEXT_MUTED
        } else if selected {
            color::TEXT_ON_ACCENT
        } else {
            color::TEXT_SECONDARY
        };
        container::Style {
            background: fill.map(Background::Color),
            text_color: Some(ink),
            border: border(0.0, Color::TRANSPARENT, radius::R8),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// The style of a [`Button`] variant in each of iced's states. Only a link
/// has a hover or pressed style; a disabled button keeps its shape and dims
/// to the muted text.
pub fn button(
    variant: Button,
) -> impl Fn(&Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_, status| {
        let disabled = matches!(status, iced::widget::button::Status::Disabled);
        let (fill, ink, edge, width, r) = match variant {
            Button::Primary => (
                Some(color::ACCENT),
                color::TEXT_ON_ACCENT,
                Color::TRANSPARENT,
                0.0,
                radius::R12,
            ),
            Button::WarningPrimary => (
                Some(color::WARNING),
                color::TEXT_ON_ACCENT,
                Color::TRANSPARENT,
                0.0,
                radius::R10,
            ),
            Button::OutlineStrong => (
                None,
                color::TEXT_PRIMARY,
                color::BORDER_STRONG,
                1.0,
                radius::R12,
            ),
            Button::Secondary => (
                None,
                color::TEXT_PRIMARY,
                color::BORDER_CONTROL,
                1.0,
                radius::R10,
            ),
            Button::Tonal => (
                Some(color::BG_RAISED),
                color::TEXT_PRIMARY,
                Color::TRANSPARENT,
                0.0,
                radius::R10,
            ),
            Button::WarningTonal => (
                Some(color::WARNING_SOFT),
                color::WARNING,
                Color::TRANSPARENT,
                0.0,
                radius::R10,
            ),
            Button::WarningOutline => (
                None,
                color::TEXT_ON_WARNING_SOFT,
                color::WARNING_BORDER,
                1.0,
                radius::R10,
            ),
            Button::Ghost => (
                None,
                color::TEXT_MUTED,
                Color::TRANSPARENT,
                0.0,
                radius::R10,
            ),
            Button::Link => (
                None,
                if matches!(status, iced::widget::button::Status::Hovered) {
                    color::ACCENT_HOVER
                } else {
                    color::ACCENT
                },
                Color::TRANSPARENT,
                0.0,
                0.0,
            ),
            Button::Nav { active } => (
                active.then_some(color::ACCENT_SOFT),
                if active {
                    color::ACCENT
                } else {
                    color::TEXT_SECONDARY
                },
                Color::TRANSPARENT,
                0.0,
                radius::R12,
            ),
            Button::Segment { selected } => (
                selected.then_some(color::ACCENT),
                if selected {
                    color::TEXT_ON_ACCENT
                } else {
                    color::TEXT_SECONDARY
                },
                Color::TRANSPARENT,
                0.0,
                radius::R8,
            ),
            Button::Dashed => (None, color::ACCENT, color::BORDER_STRONG, 1.0, radius::R12),
            Button::Bare => (None, color::TEXT_PRIMARY, Color::TRANSPARENT, 0.0, 0.0),
        };
        let fill = if disabled && fill.is_some() {
            Some(color::BG_RAISED)
        } else {
            fill
        };
        let ink = if disabled { color::TEXT_MUTED } else { ink };
        iced::widget::button::Style {
            background: fill.map(Background::Color),
            text_color: ink,
            border: border(width, edge, r),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

// -------------------------------------------------------------------- fields

/// A text field: the page colour inside the control border. A focused field
/// draws its border in the accent so a keyboard user can see the focus.
pub fn field(r: f32) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let edge = match status {
            text_input::Status::Focused { .. } => color::ACCENT,
            _ => color::BORDER_CONTROL,
        };
        text_input::Style {
            background: Background::Color(color::BG_APP),
            border: border(1.0, edge, r),
            icon: color::TEXT_MUTED,
            placeholder: color::PLACEHOLDER,
            value: color::TEXT_PRIMARY,
            selection: color::ACCENT_BORDER,
        }
    }
}

/// A text field inside a box that draws its own border (the search fields):
/// transparent and borderless.
pub fn bare_field(_: &Theme, _: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: border(0.0, Color::TRANSPARENT, 0.0),
        icon: color::TEXT_MUTED,
        placeholder: color::PLACEHOLDER,
        value: color::TEXT_PRIMARY,
        selection: color::ACCENT_BORDER,
    }
}

/// A progress bar: the pending step's track and the accent fill.
pub fn progress(_: &Theme) -> iced::widget::progress_bar::Style {
    iced::widget::progress_bar::Style {
        background: Background::Color(color::BORDER_CONTROL),
        bar: Background::Color(color::ACCENT),
        border: border(0.0, Color::TRANSPARENT, radius::R2),
    }
}

// ------------------------------------------------------------- rules, scroll

/// A divider: one pixel of the subtle border colour.
pub fn divider(_: &Theme) -> rule::Style {
    rule::Style {
        color: color::BORDER_SUBTLE,
        radius: border::Radius::default(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

/// A scrollable that draws nothing but a thin rail when it scrolls.
pub fn scroll(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    let rail = |mut r: scrollable::Rail| {
        r.background = None;
        r.border = border(0.0, Color::TRANSPARENT, 0.0);
        r.scroller.background = Background::Color(color::BORDER_CONTROL);
        r.scroller.border = border(0.0, Color::TRANSPARENT, radius::R2);
        r
    };
    style.vertical_rail = rail(style.vertical_rail);
    style.horizontal_rail = rail(style.horizontal_rail);
    style.container = container::Style::default();
    style
}
