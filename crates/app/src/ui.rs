// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The pieces every screen is built from: text in the type scale, buttons,
//! fields, cards, chips and the library's pages.

use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{Button, Text, button, column, container, row, rule, space, text, text_input};
use iced::{Alignment, Color, Element, Font, Length, Padding, Pixels};

use crate::fonts;
use crate::icon::{self, Icon};
use crate::tap::tap;
use crate::theme::{self, color, radius, space as sp};

/// A role in the type scale: a face and a size. The line height follows
/// from the face: Poppins 1.5, Montserrat 1.219, IBM Plex Mono 1.3.
#[derive(Clone, Copy, Debug)]
pub struct Type {
    pub font: Font,
    pub size: f32,
}

impl Type {
    const fn new(font: Font, size: f32) -> Type {
        Type { font, size }
    }

    fn line_height(self) -> f32 {
        match self.font.family {
            iced::font::Family::Name("Poppins") => 1.5,
            iced::font::Family::Name("Montserrat") => 1.219,
            _ => 1.3,
        }
    }
}

/// The type scale, by role.
pub mod ty {
    use super::Type;
    use crate::fonts;

    pub const HERO: Type = Type::new(fonts::DISPLAY_BOLD, 52.0);
    pub const HERO_ITALIC: Type = Type::new(fonts::DISPLAY_BOLD_ITALIC, 52.0);
    /// The welcome screen's headline on a phone. At [`HERO`]'s 52 points the
    /// second line wraps at 360 wide and the screen no longer fits 640 pixels
    /// of height; at 34 it stays on one line and the screen fits.
    pub const HERO_PHONE: Type = Type::new(fonts::DISPLAY_BOLD, 34.0);
    /// The italic second line of [`HERO_PHONE`].
    pub const HERO_PHONE_ITALIC: Type = Type::new(fonts::DISPLAY_BOLD_ITALIC, 34.0);
    pub const ONBOARDING_TITLE: Type = Type::new(fonts::DISPLAY_BOLD, 28.0);
    pub const PAGE_TITLE: Type = Type::new(fonts::DISPLAY_BOLD, 26.0);
    pub const WORDMARK: Type = Type::new(fonts::DISPLAY_BOLD, 22.0);
    pub const WORDMARK_LARGE: Type = Type::new(fonts::DISPLAY_BOLD, 30.0);
    pub const SUBTITLE: Type = Type::new(fonts::BODY, 13.0);
    pub const INTRO: Type = Type::new(fonts::BODY, 14.0);
    pub const CARD_TITLE: Type = Type::new(fonts::DISPLAY_BOLD, 16.0);
    pub const SECTION_LABEL: Type = Type::new(fonts::DISPLAY_SEMIBOLD, 12.0);
    pub const TABLE_HEADER: Type = Type::new(fonts::BODY_MEDIUM, 12.0);
    pub const TABLE_BODY: Type = Type::new(fonts::BODY, 13.0);
    pub const TABLE_NAME: Type = Type::new(fonts::BODY_MEDIUM, 13.0);
    pub const BODY: Type = Type::new(fonts::BODY, 14.0);
    pub const BODY_SMALL: Type = Type::new(fonts::BODY, 13.0);
    pub const ROW_TITLE: Type = Type::new(fonts::BODY_MEDIUM, 14.0);
    pub const NOTE: Type = Type::new(fonts::BODY, 12.0);
    pub const TINY: Type = Type::new(fonts::BODY, 11.0);
    pub const FORM_LABEL: Type = Type::new(fonts::BODY_MEDIUM, 12.0);
    pub const RADIO_TITLE: Type = Type::new(fonts::DISPLAY_BOLD, 15.0);
    pub const RADIO_TEXT: Type = Type::new(fonts::BODY, 13.0);
    pub const MONO: Type = Type::new(fonts::MONO, 13.0);
    pub const MONO_SMALL: Type = Type::new(fonts::MONO, 12.0);
    pub const MONO_TINY: Type = Type::new(fonts::MONO, 11.0);
    pub const MONO_WORD: Type = Type::new(fonts::MONO_MEDIUM, 15.0);
    /// A word of the recovery phrase in its grid: 14 pixels, one less than
    /// [`MONO_WORD`], because at 15 the widest word of the library's list,
    /// "abstract", would not fit its cell on a phone 360 wide.
    pub const PHRASE_WORD: Type = Type::new(fonts::MONO_MEDIUM, 14.0);
    pub const TABLE_AMOUNT: Type = Type::new(fonts::DISPLAY_SEMIBOLD, 13.0);
    pub const STAT_VALUE: Type = Type::new(fonts::DISPLAY_BOLD, 20.0);
    pub const HAIKU: Type = Type::new(fonts::DISPLAY_MEDIUM_ITALIC, 26.0);
    pub const BALANCE: Type = Type::new(fonts::DISPLAY_BOLD, 56.0);
    pub const BALANCE_DECIMALS: Type = Type::new(fonts::DISPLAY_BOLD, 26.0);
    pub const BALANCE_UNIT: Type = Type::new(fonts::DISPLAY_BOLD, 18.0);
    pub const BUTTON_XL: Type = Type::new(fonts::DISPLAY_BOLD, 15.0);
    pub const BUTTON_HEADER: Type = Type::new(fonts::DISPLAY_BOLD, 14.0);
    pub const BUTTON_SECONDARY: Type = Type::new(fonts::BODY_MEDIUM, 13.0);
    pub const BUTTON_SMALL: Type = Type::new(fonts::BODY_MEDIUM, 12.0);
    pub const LINK: Type = Type::new(fonts::BODY_MEDIUM, 13.0);
    pub const LINK_SMALL: Type = Type::new(fonts::BODY_MEDIUM, 12.0);
    pub const NAV: Type = Type::new(fonts::BODY_MEDIUM, 14.0);
    pub const NAV_ACTIVE: Type = Type::new(fonts::BODY_SEMIBOLD, 14.0);
    /// The phone's bar at the foot of the window, which holds the six items
    /// in one row.
    pub const NAV_TAB: Type = Type::new(fonts::BODY_MEDIUM, 11.0);
    pub const NAV_TAB_ACTIVE: Type = Type::new(fonts::BODY_SEMIBOLD, 11.0);
    pub const CHIP: Type = Type::new(fonts::BODY_MEDIUM, 12.0);
    pub const BADGE: Type = Type::new(fonts::BODY_SEMIBOLD, 11.0);
    pub const PILL: Type = Type::new(fonts::BODY_MEDIUM, 13.0);
    pub const CALLOUT_TITLE: Type = Type::new(fonts::DISPLAY_SEMIBOLD, 14.0);
    pub const CALLOUT_BODY: Type = Type::new(fonts::BODY, 12.0);
    pub const FIELD: Type = Type::new(fonts::BODY, 15.0);
    pub const PAGE: Type = Type::new(fonts::MONO, 12.0);
}

/// `content` in role `ty` and `color`.
pub fn t<'a>(content: impl text::IntoFragment<'a>, ty: Type, color: Color) -> Text<'a> {
    text(content)
        .font(ty.font)
        .size(ty.size)
        .line_height(LineHeight::Relative(ty.line_height()))
        .color(color)
}

/// `content` as [`t`] sets it, wrapping at words or glyphs, with each "→"
/// drawn from [`fonts::ARROW`] when `ty` is Poppins: Poppins has no arrow,
/// and one drawn from whatever system font the machine has would make the
/// screenshots differ from machine to machine.
pub fn arrowed<'a, M: 'a>(content: String, ty: Type, color: Color) -> Element<'a, M> {
    let poppins = ty.font.family == iced::font::Family::Name("Poppins");
    if !poppins || !content.contains('→') {
        return t(content, ty, color).wrapping(Wrapping::WordOrGlyph).into();
    }
    let mut spans: Vec<iced::widget::text::Span<'a, (), Font>> = Vec::new();
    for (n, part) in content.split('→').enumerate() {
        if n > 0 {
            spans.push(iced::widget::span("→").font(fonts::ARROW));
        }
        if !part.is_empty() {
            spans.push(iced::widget::span(part.to_owned()));
        }
    }
    iced::widget::rich_text(spans)
        .font(ty.font)
        .size(ty.size)
        .line_height(LineHeight::Relative(ty.line_height()))
        .color(color)
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

/// `content` in role `ty`, in the colour of whatever it sits in (a
/// button's label takes the button's text colour).
pub fn label<'a>(content: impl text::IntoFragment<'a>, ty: Type) -> Text<'a> {
    text(content)
        .font(ty.font)
        .size(ty.size)
        .line_height(LineHeight::Relative(ty.line_height()))
}

/// A section label: Montserrat 12/600, upper case, muted. Set solid, since
/// iced has no letter spacing.
pub fn section_label<'a>(content: &str) -> Text<'a> {
    t(content.to_uppercase(), ty::SECTION_LABEL, color::TEXT_MUTED)
}

/// A page title over its subtitle.
pub fn page_header<'a, M: 'a>(
    title: &str,
    subtitle: impl text::IntoFragment<'a>,
) -> Element<'a, M> {
    // A page with nothing to say under its title draws no subtitle line.
    let subtitle = subtitle.into_fragment();
    let mut header = column![t(title.to_uppercase(), ty::PAGE_TITLE, color::TEXT_PRIMARY)];
    if !subtitle.is_empty() {
        header = header.push(t(subtitle, ty::SUBTITLE, color::TEXT_MUTED));
    }
    header.spacing(sp::S2).into()
}

/// The button sizes the screens use.
#[derive(Clone, Copy, Debug)]
pub enum Size {
    /// 56 px, full width ("Continue" on the first-run screens).
    Xl,
    /// 44 px (page header buttons).
    Header,
    /// 40 px.
    Medium,
    /// 36 px.
    Small,
}

impl Size {
    fn height(self) -> f32 {
        match self {
            Size::Xl => 56.0,
            Size::Header => 44.0,
            Size::Medium => 40.0,
            Size::Small => 36.0,
        }
    }
}

/// A button of `variant` and `size`, labelled `text`, with an optional
/// leading icon. `on_press` of `None` draws it disabled.
pub fn button_with<'a, M: Clone + 'a>(
    content: impl text::IntoFragment<'a>,
    variant: theme::Button,
    size: Size,
    lead: Option<Icon>,
    on_press: Option<M>,
) -> Button<'a, M> {
    let ink = button_ink(variant, on_press.is_some());
    let face = match (variant, size) {
        (theme::Button::Primary | theme::Button::WarningPrimary, Size::Xl) => ty::BUTTON_XL,
        (theme::Button::Primary | theme::Button::WarningPrimary, _) => ty::BUTTON_HEADER,
        (theme::Button::OutlineStrong, _) => Type::new(fonts::DISPLAY_SEMIBOLD, 14.0),
        (_, Size::Small) if matches!(variant, theme::Button::WarningTonal) => ty::BUTTON_SMALL,
        _ => ty::BUTTON_SECONDARY,
    };
    let mut inner = row![].spacing(sp::S8).align_y(Alignment::Center);
    if let Some(i) = lead {
        inner = inner.push(icon::icon(i, 16.0, 2.0, ink));
    }
    inner = inner.push(label(content, face));
    let padding = match size {
        Size::Xl => Padding::from([0.0, sp::S20]),
        Size::Header => Padding::from([0.0, sp::S20]),
        _ => Padding::from([0.0, sp::S14]),
    };
    let width = if matches!(size, Size::Xl) {
        Length::Fill
    } else {
        Length::Shrink
    };
    button(
        container(inner)
            .center_y(Length::Fill)
            .align_x(if matches!(size, Size::Xl) {
                Alignment::Center
            } else {
                Alignment::Start
            })
            .width(width),
    )
    .height(Length::Fixed(size.height()))
    .width(width)
    .padding(padding)
    .style(theme::button(variant))
    .on_press_maybe(on_press)
}

/// The text colour a button of `variant` draws its icon in, as
/// [`theme::button`] draws its label.
fn button_ink(variant: theme::Button, enabled: bool) -> Color {
    if !enabled {
        return color::TEXT_MUTED;
    }
    match variant {
        theme::Button::Primary | theme::Button::WarningPrimary => color::TEXT_ON_ACCENT,
        theme::Button::WarningTonal => color::WARNING,
        theme::Button::WarningOutline => color::TEXT_ON_WARNING_SOFT,
        theme::Button::Ghost => color::TEXT_MUTED,
        theme::Button::Link | theme::Button::Dashed => color::ACCENT,
        theme::Button::Nav { active } => {
            if active {
                color::ACCENT
            } else {
                color::TEXT_SECONDARY
            }
        }
        theme::Button::Segment { selected } => {
            if selected {
                color::TEXT_ON_ACCENT
            } else {
                color::TEXT_SECONDARY
            }
        }
        theme::Button::OutlineStrong
        | theme::Button::Secondary
        | theme::Button::Tonal
        | theme::Button::Bare => color::TEXT_PRIMARY,
    }
}

/// A text link.
pub fn link<'a, M: Clone + 'a>(content: &'a str, ty: Type, on_press: M) -> Button<'a, M> {
    button(label(content, ty))
        .padding(0)
        .style(theme::button(theme::Button::Link))
        .on_press(on_press)
}

/// A segment of a segmented control or a filter row: drawn as
/// `button_with(content, theme::Button::Segment { selected }, Size::Small,
/// None, on_tap)` draws it, and acting on release, as a [`tap`] does.
/// `width` is the segment's own.
pub fn segment<'a, M: Clone + 'a>(
    content: &'a str,
    selected: bool,
    width: Length,
    on_tap: Option<M>,
) -> Element<'a, M> {
    let enabled = on_tap.is_some();
    let segment = container(
        container(row![label(content, ty::BUTTON_SECONDARY)].align_y(Alignment::Center))
            .center_y(Length::Fill)
            .center_x(width),
    )
    .height(Length::Fixed(Size::Small.height()))
    .width(width)
    .padding(Padding::from([0.0, sp::S14]))
    .style(theme::segment(selected, enabled));
    tap(segment).on_tap_maybe(on_tap).into()
}

/// A labelled text field: the label over the field, 6 px apart.
pub fn field<'a, M: Clone + 'a>(
    field_label: impl text::IntoFragment<'a>,
    placeholder: &'a str,
    value: &'a str,
    face: Type,
    secure: bool,
    on_input: impl Fn(String) -> M + 'a,
    on_submit: Option<M>,
) -> Element<'a, M> {
    let mut input = text_input(placeholder, value)
        .font(face.font)
        .size(face.size)
        .padding(Padding::from([sp::S12, sp::S14]))
        .secure(secure)
        .style(theme::field(radius::R12))
        .on_input(on_input);
    if let Some(m) = on_submit {
        input = input.on_submit(m);
    }
    column![t(field_label, ty::FORM_LABEL, color::TEXT_SECONDARY), input]
        .spacing(sp::S6)
        .into()
}

/// Library text broken for a terminal, joined again within each
/// paragraph: a single line break becomes a space, a blank line stays. No
/// word is changed.
#[must_use]
pub fn reflow(text: &str) -> String {
    text.trim()
        .split("\n\n")
        .map(|p| p.split('\n').map(str::trim).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Helper text under a field: 12 px, muted.
pub fn helper<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    t(content, ty::NOTE, color::TEXT_MUTED)
}

/// A refusal under a form, in the library's or the worker's words, whole.
pub fn refusal<'a, M: 'a>(content: &'a str) -> Element<'a, M> {
    container(
        row![
            icon::icon(Icon::Warning, 18.0, 2.0, color::WARNING),
            t(content, ty::CALLOUT_BODY, color::TEXT_ON_WARNING_SOFT)
                .line_height(LineHeight::Relative(1.5))
                .width(Length::Fill),
        ]
        .spacing(sp::S12)
        .align_y(Alignment::Start),
    )
    .padding(sp::S14)
    .width(Length::Fill)
    .style(theme::callout_warning)
    .into()
}

/// An amber callout with a title.
pub fn warning_callout<'a, M: 'a>(
    title: &'a str,
    body: impl text::IntoFragment<'a>,
) -> Element<'a, M> {
    container(
        row![
            container(icon::icon(Icon::Warning, 20.0, 2.0, color::WARNING))
                .center(Length::Fixed(40.0))
                .style(theme::icon_tile(color::WARNING_SOFT, color::WARNING)),
            column![
                t(title, ty::CALLOUT_TITLE, color::TEXT_PRIMARY),
                t(body, ty::CALLOUT_BODY, color::TEXT_ON_WARNING_SOFT)
                    .line_height(LineHeight::Relative(1.5)),
            ]
            .spacing(sp::S4)
            .width(Length::Fill),
        ]
        .spacing(sp::S16)
        .align_y(Alignment::Start),
    )
    .padding(sp::S18)
    .width(Length::Fill)
    .style(theme::callout_warning)
    .into()
}

/// An amber callout with no title: [`warning_callout`]'s icon and body alone.
pub fn warning_note<'a, M: 'a>(body: impl text::IntoFragment<'a>) -> Element<'a, M> {
    container(
        row![
            container(icon::icon(Icon::Warning, 20.0, 2.0, color::WARNING))
                .center(Length::Fixed(40.0))
                .style(theme::icon_tile(color::WARNING_SOFT, color::WARNING)),
            t(body, ty::CALLOUT_BODY, color::TEXT_ON_WARNING_SOFT)
                .line_height(LineHeight::Relative(1.5))
                .width(Length::Fill),
        ]
        .spacing(sp::S16)
        .align_y(Alignment::Center),
    )
    .padding(sp::S18)
    .width(Length::Fill)
    .style(theme::callout_warning)
    .into()
}

/// A green callout with an icon and a title.
pub fn accent_callout<'a, M: 'a>(
    lead: Icon,
    title: impl text::IntoFragment<'a>,
    body: impl text::IntoFragment<'a>,
) -> Element<'a, M> {
    container(
        row![
            container(icon::icon(lead, 20.0, 2.2, color::TEXT_ON_ACCENT))
                .center(Length::Fixed(40.0))
                .style(theme::icon_tile(color::ACCENT, color::TEXT_ON_ACCENT)),
            column![
                t(title, ty::CALLOUT_TITLE, color::TEXT_PRIMARY),
                t(body, ty::CALLOUT_BODY, color::TEXT_ON_ACCENT_SOFT)
                    .line_height(LineHeight::Relative(1.5)),
            ]
            .spacing(sp::S4)
            .width(Length::Fill),
        ]
        .spacing(sp::S14)
        .align_y(Alignment::Start),
    )
    .padding(sp::S18)
    .width(Length::Fill)
    .style(theme::callout_accent)
    .into()
}

/// [`accent_callout`], with `actions` (a row of buttons) beneath its text and
/// inside the callout, 14 pixels below the body, as the device unlock's offer
/// uses.
pub fn accent_callout_with_actions<'a, M: 'a>(
    lead: Icon,
    title: impl text::IntoFragment<'a>,
    body: impl text::IntoFragment<'a>,
    actions: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    container(
        row![
            container(icon::icon(lead, 20.0, 2.2, color::TEXT_ON_ACCENT))
                .center(Length::Fixed(40.0))
                .style(theme::icon_tile(color::ACCENT, color::TEXT_ON_ACCENT)),
            column![
                column![
                    t(title, ty::CALLOUT_TITLE, color::TEXT_PRIMARY),
                    t(body, ty::CALLOUT_BODY, color::TEXT_ON_ACCENT_SOFT)
                        .line_height(LineHeight::Relative(1.5)),
                ]
                .spacing(sp::S4),
                actions.into(),
            ]
            .spacing(sp::S14)
            .width(Length::Fill),
        ]
        .spacing(sp::S14)
        .align_y(Alignment::Start),
    )
    .padding(sp::S18)
    .width(Length::Fill)
    .style(theme::callout_accent)
    .into()
}

/// The command-line verbs a library page can name, and the control in
/// this application that does the same. Each is found by the verb as the
/// page quotes it, in backticks.
const VERBS: [(&str, &str); 9] = [
    (
        "--advance-to",
        "`reconcile ... --advance-to N` is the account-recovery screen, which shows every \
         account's report first and asks you to type the index.",
    ),
    ("`settle", "`settle` is Settle, on the account's page."),
    ("`resign", "`resign` is Re-sign, on the account's page."),
    ("`submit", "`submit` is Submit saved artifact."),
    ("`restore", "`restore --account N` is Add account."),
    ("`discover", "`discover` is Discover accounts."),
    ("`status", "`status` is Check now, on the account's page."),
    // With its argument: the receive page itself quotes `address` for the
    // ledger line it shows, and that is not the verb.
    ("`address ", "`address` is Receive."),
    ("`create", "`create` is making a new wallet."),
];

/// The notes for the command-line verbs `page` names, each naming the
/// control in Tawara that does the same.
#[must_use]
pub fn verb_notes(page: &str) -> Vec<&'static str> {
    VERBS
        .iter()
        .filter(|(needle, _)| page.contains(needle))
        .map(|(_, note)| *note)
        .collect()
}

/// One of the library's pages, word for word: IBM Plex Mono, its line
/// breaks kept, never cut short; then, where
/// it names a command-line verb, which control does the same.
pub fn library_page<'a, M: 'a>(page: &'a str) -> Element<'a, M> {
    let mut body = column![
        t(page.trim_end(), ty::PAGE, color::TEXT_SECONDARY)
            .wrapping(Wrapping::WordOrGlyph)
            .width(Length::Fill)
    ]
    .spacing(sp::S14);
    let notes = verb_notes(page);
    if !notes.is_empty() {
        let mut list = column![t(
            "This page is the wallet library's, word for word. Where it names a command, \
             the control in Tawara that does the same:",
            ty::NOTE,
            color::TEXT_MUTED,
        )]
        .spacing(sp::S4);
        for note in notes {
            list = list.push(t(note, ty::NOTE, color::TEXT_SECONDARY));
        }
        body = body.push(divider()).push(list);
    }
    container(body)
        .padding(sp::S20)
        .width(Length::Fill)
        .style(theme::card)
        .into()
}

/// A pill: `content` on `fill` in `ink`, `height` tall.
pub fn pill<'a, M: 'a>(
    content: impl text::IntoFragment<'a>,
    face: Type,
    fill: Color,
    ink: Color,
    height: f32,
) -> Element<'a, M> {
    container(label(content, face))
        .height(Length::Fixed(height))
        .padding(Padding::from([0.0, height / 2.6]))
        .center_y(Length::Fixed(height))
        .style(theme::pill(fill, ink))
        .into()
}

/// A status dot, `size` pixels across.
pub fn dot<'a, M: 'a>(fill: Color, size: f32) -> Element<'a, M> {
    container(space())
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(theme::pill(fill, fill))
        .into()
}

/// A one-pixel divider.
pub fn divider<'a, M: 'a>() -> Element<'a, M> {
    rule::horizontal(1).style(theme::divider).into()
}

/// A card: surface, border, radius 20, padding 24.
pub fn card<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> container::Container<'a, M> {
    card_for(false, content)
}

/// A card as [`card`] draws it, with its padding 16 in the phone layout, so
/// that the card fills the width of a phone screen.
pub fn card_for<'a, M: 'a>(
    compact: bool,
    content: impl Into<Element<'a, M>>,
) -> container::Container<'a, M> {
    container(content)
        .padding(if compact { sp::S16 } else { sp::S24 })
        .width(Length::Fill)
        .style(theme::card)
}

/// The amount `nano` laid out for the interface: MCM with thousands grouped
/// and no low-order zeros (`12,480.537214906`, `3.2`, `500`), or nanoMCM
/// grouped (`12,480,537,214,906`).
#[must_use]
pub fn amount(nano: impl Into<u128>, unit: tawara_wallet_core::preferences::AmountUnit) -> String {
    use tawara_wallet_core::preferences::AmountUnit;
    let nano: u128 = nano.into();
    match unit {
        AmountUnit::Mcm => tawara_wallet_core::amount::show_mcm(nano),
        AmountUnit::NanoMcm => group(&nano.to_string()),
    }
}

/// `digits` with a comma every three from the right.
#[must_use]
pub fn group(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The unit's own name, as set beside an amount.
#[must_use]
pub fn unit_name(unit: tawara_wallet_core::preferences::AmountUnit) -> &'static str {
    match unit {
        tawara_wallet_core::preferences::AmountUnit::Mcm => "MCM",
        tawara_wallet_core::preferences::AmountUnit::NanoMcm => "nanoMCM",
    }
}

/// Which clock the screens tell a time by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Zone {
    /// The system's time zone, with its summer time, as chrono reads it:
    /// the TZ variable or /etc/localtime on Unix, the system's API on
    /// Windows, the tz data on Android.
    #[default]
    System,
    /// A fixed offset from UTC, in seconds east: the screenshots, so they
    /// are drawn the same on every machine.
    Fixed(i32),
}

/// A time the node gave (milliseconds since the epoch) as the wall clock
/// read it in `zone`. The library carries no calendar and prints the raw
/// count; `None` for a count no calendar holds.
fn wall(ms: i64, zone: Zone) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let at = chrono::DateTime::from_timestamp_millis(ms)?;
    let offset = match zone {
        Zone::System => *at.with_timezone(&chrono::Local).offset(),
        Zone::Fixed(east) => chrono::FixedOffset::east_opt(east)?,
    };
    Some(at.with_timezone(&offset))
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// "Oct 3", on the wall clock.
#[must_use]
pub fn short_date(ms: i64, zone: Zone) -> String {
    use chrono::Datelike as _;
    wall(ms, zone).map_or_else(
        || "—".to_owned(),
        |t| format!("{} {}", MONTHS[t.month0() as usize], t.day()),
    )
}

/// "Oct 3 · 14:03", on the wall clock.
#[must_use]
pub fn date_time(ms: i64, zone: Zone) -> String {
    use chrono::Timelike as _;
    wall(ms, zone).map_or_else(
        || "—".to_owned(),
        |t| {
            format!(
                "{} · {:02}:{:02}",
                short_date(ms, zone),
                t.hour(),
                t.minute()
            )
        },
    )
}

/// "2026-10-03 16:03:20 UTC+2", on the wall clock, saying its offset.
#[must_use]
pub fn full_time(ms: i64, zone: Zone) -> String {
    use chrono::{Datelike as _, Timelike as _};
    wall(ms, zone).map_or_else(
        || "—".to_owned(),
        |t| {
            format!(
                "{}-{:02}-{:02} {:02}:{:02}:{:02} {}",
                t.year(),
                t.month(),
                t.day(),
                t.hour(),
                t.minute(),
                t.second(),
                offset_name(t.offset().local_minus_utc())
            )
        },
    )
}

/// An offset from UTC as a person reads it: "UTC", "UTC+2", "UTC-5:30".
fn offset_name(east: i32) -> String {
    if east == 0 {
        return "UTC".to_owned();
    }
    let sign = if east > 0 { '+' } else { '-' };
    let (hours, minutes) = (east.abs() / 3_600, east.abs() % 3_600 / 60);
    if minutes == 0 {
        format!("UTC{sign}{hours}")
    } else {
        format!("UTC{sign}{hours}:{minutes:02}")
    }
}

/// How long before `now` the time `then` was, both in milliseconds since
/// the epoch: "48 s ago", "6 min ago", "3 h ago", "2 days ago". A time
/// after `now` (a clock behind the node's) is "just now".
#[must_use]
pub fn age(now: i64, then: i64) -> String {
    let secs = now.saturating_sub(then) / 1_000;
    match secs {
        ..=0 => "just now".to_owned(),
        1..=99 => format!("{secs} s ago"),
        100..=5_999 => format!("{} min ago", secs / 60),
        6_000..=172_799 => format!("{} h ago", secs / 3_600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// Fixed pixels, for the layout's stated sizes.
#[must_use]
pub fn px(n: f32) -> Length {
    Length::Fixed(n)
}

/// A size in pixels, for iced's text sizes.
#[must_use]
pub fn pixels(n: f32) -> Pixels {
    Pixels(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tawara_wallet_core::preferences::AmountUnit;

    #[test]
    fn verbs_the_library_names_get_their_control() {
        let page = "after reading the report above, `reconcile 0x5d --advance-to 33` takes \
                    the acknowledged path; `settle` it afterwards";
        assert_eq!(verb_notes(page).len(), 2);
        assert!(verb_notes("nothing to say").is_empty());
    }

    #[test]
    fn reflow_joins_lines_and_keeps_paragraphs() {
        assert_eq!(reflow("a b\nc d\n\ne\nf\n"), "a b c d\n\ne f");
    }

    #[test]
    fn amounts_are_grouped_and_exact() {
        assert_eq!(
            amount(12_480_537_214_906_u64, AmountUnit::Mcm),
            "12,480.537214906"
        );
        // Low-order zeros are left out; nothing is rounded.
        assert_eq!(amount(500_u64, AmountUnit::Mcm), "0.0000005");
        assert_eq!(
            amount(9_215_402_118_000_u64, AmountUnit::Mcm),
            "9,215.402118"
        );
        assert_eq!(amount(41_250_000_000_000_u64, AmountUnit::Mcm), "41,250");
        assert_eq!(amount(0_u64, AmountUnit::Mcm), "0");
        assert_eq!(
            amount(12_480_537_214_906_u64, AmountUnit::NanoMcm),
            "12,480,537,214,906"
        );
        assert_eq!(amount(0_u64, AmountUnit::NanoMcm), "0");
        // A total past u64 is written exactly, not clamped.
        assert_eq!(
            amount(u128::from(u64::MAX) + 5, AmountUnit::Mcm),
            "18,446,744,073.70955162"
        );
        assert_eq!(group("100"), "100");
        assert_eq!(group("1000"), "1,000");
    }

    #[test]
    fn times_are_told_on_the_wall_clock_of_the_zone() {
        let utc = Zone::Fixed(0);
        assert_eq!(full_time(0, utc), "1970-01-01 00:00:00 UTC");
        assert_eq!(full_time(1_791_036_200_000, utc), "2026-10-03 14:03:20 UTC");
        assert_eq!(date_time(1_791_036_200_000, utc), "Oct 3 · 14:03");
        assert_eq!(full_time(951_782_400_000, utc), "2000-02-29 00:00:00 UTC");
        assert_eq!(full_time(-1_000, utc), "1969-12-31 23:59:59 UTC");
        // East of UTC the date can turn over; west of it, back.
        let tokyo = Zone::Fixed(9 * 3_600);
        assert_eq!(
            full_time(1_791_036_200_000, tokyo),
            "2026-10-03 23:03:20 UTC+9"
        );
        assert_eq!(date_time(1_791_050_000_000, tokyo), "Oct 4 · 02:53");
        let delhi = Zone::Fixed(5 * 3_600 + 1_800);
        assert!(full_time(0, delhi).ends_with("UTC+5:30"));
        let la = Zone::Fixed(-7 * 3_600);
        assert_eq!(short_date(1_791_000_000_000, la), "Oct 2");
        // The system's zone gives a time too, whichever it is.
        assert_ne!(full_time(1_791_036_200_000, Zone::System), "—");
        assert_eq!(age(10_000, 10_000), "just now");
        assert_eq!(age(58_000, 10_000), "48 s ago");
        assert_eq!(age(10_000 + 360_000, 10_000), "6 min ago");
        assert_eq!(age(3 * 86_400_000, 0), "3 days ago");
        assert_eq!(age(0, 5_000), "just now", "a clock behind the API's");
    }

    /// What `root` draws at `size`, as RGBA pixels.
    fn drawn<'a>(root: impl Into<Element<'a, i32>>, size: (u32, u32)) -> Vec<u8> {
        crate::harness::Driver::new(root, size).draw()
    }

    #[test]
    fn a_segment_draws_the_pixels_of_the_button_it_replaces() {
        for selected in [false, true] {
            for enabled in [false, true] {
                let on_tap = enabled.then_some(1);
                let segment = segment("Week", selected, Length::Shrink, on_tap);
                let button = button_with(
                    "Week",
                    theme::Button::Segment { selected },
                    Size::Small,
                    None,
                    on_tap,
                );
                assert!(
                    drawn(segment, (160, 60)) == drawn(button, (160, 60)),
                    "selected {selected}, enabled {enabled}: the pixels differ"
                );
            }
        }
    }

    #[test]
    fn a_tap_over_a_card_draws_the_pixels_of_a_bare_button_over_it() {
        for selected in [false, true] {
            let content = || {
                iced::widget::column![
                    t("Account", ty::RADIO_TITLE, color::TEXT_PRIMARY),
                    t(
                        "9xQmT4vRbN2kWe7HpLsZ3cYdAf",
                        ty::MONO_TINY,
                        color::TEXT_MUTED
                    ),
                ]
                .spacing(sp::S2)
            };
            let tapped: Element<'_, i32> = crate::tap::tap(
                container(content())
                    .padding(Padding::from([sp::S12, sp::S16]))
                    .width(Length::Fill)
                    .style(theme::radio_card(selected)),
            )
            .on_tap(1)
            .into();
            let bare: Element<'_, i32> = button(
                container(content())
                    .padding(Padding::from([sp::S12, sp::S16]))
                    .width(Length::Fill)
                    .style(theme::radio_card(selected)),
            )
            .padding(0)
            .width(Length::Fill)
            .style(theme::button(theme::Button::Bare))
            .on_press(1)
            .into();
            assert!(
                drawn(tapped, (320, 90)) == drawn(bare, (320, 90)),
                "selected {selected}: the pixels differ"
            );
        }
    }

    #[test]
    fn card_for_is_the_card_on_the_desktop_and_sixteen_padded_on_a_phone() {
        let body = || t("body", ty::BODY, color::TEXT_PRIMARY);
        assert!(
            drawn(card(body()), (200, 100)) == drawn(card_for(false, body()), (200, 100)),
            "the desktop card is unchanged"
        );
        assert!(
            drawn(card_for(true, body()), (200, 100)) != drawn(card(body()), (200, 100)),
            "the phone's card has the smaller padding"
        );
    }
}
