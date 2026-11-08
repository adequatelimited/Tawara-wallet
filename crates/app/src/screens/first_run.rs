// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! First run and unlocking: the brand panel on the left and the form on the
//! right. On a phone the brand panel is not drawn: the form is one page under
//! the wordmark. The one exception is the welcome screen, which a phone
//! without a store shows as the brand panel whole, before the start screen.

use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{checkbox, column, container, responsive, row, scrollable, space, text_input};
use iced::{Alignment, Element, Length, Padding};
use tawara_wallet_core::MIN_PASSWORD_LEN;
use tawara_wallet_core::preferences::DEFAULT_NODE;

use crate::app::{
    Back, Busy, Go, HELD_BODY, HELD_TITLE, Message, Model, NodeForm, PasswordForm, PasteInto,
    PhraseState, RestoreForm, Screen, StartChoice, Typed, UnlockForm,
};
use crate::fonts;
use crate::paste;
use crate::tap::tap;
use crate::theme::{self, color, radius, space as sp};
use crate::ui::{self, Size, Type, t, ty};

/// The first-run and unlock screens.
pub fn view(model: &Model) -> Element<'_, Message> {
    let words = match &model.screen {
        Screen::Phrase(p) => p.phrase.words().collect(),
        _ => Vec::new(),
    };
    view_of(model, words)
}

/// [`view`], with the phrase screen's `words` given beside the model rather
/// than read from its phrase. The phrase comes only from the worker, so this
/// lets the tests lay the phrase screen out with the widest words of the
/// library's list.
fn view_of<'a>(model: &'a Model, words: Vec<&'a str>) -> Element<'a, Message> {
    // A phone's recovery phrase picks its frame by the height its page is
    // given, which only the layout knows, so it is built inside `responsive`.
    // It has no wordmark, so it is not wrapped by `phone_frame`.
    if let (None, Screen::Phrase(p)) = (&model.busy, &model.screen)
        && model.compact()
    {
        let captured = model.captured;
        return responsive(move |size| phone_phrase(&words, p, size.height, captured)).into();
    }
    let phrase_screen = matches!((&model.busy, &model.screen), (None, Screen::Phrase(_)));
    let form: Element<'_, Message> = match (&model.busy, &model.screen) {
        (Some(busy), _) => opening(busy),
        // The welcome screen is the brand panel whole, on a phone alone.
        (None, Screen::Welcome) => return welcome(),
        (None, Screen::Start { choice }) => start(model, *choice),
        (None, Screen::Node(f)) => node(model, f),
        (None, Screen::NewWallet(f)) => new_wallet(model, f),
        (None, Screen::Phrase(p)) => phrase_content(
            &words,
            p.written,
            PhraseFrame::Whole,
            wide_shot(model, p),
            model.captured,
        ),
        (None, Screen::Confirm(p)) => confirm(p, model.drawn()),
        (None, Screen::Restore(r)) => restore(model, r, model.drawn()),
        (None, Screen::Unlock(u)) => unlock(model, u),
        (None, Screen::Wallet(_)) => column![].into(),
    };
    if model.compact() {
        return phone_frame(form);
    }
    // The desktop's phrase keeps 24 pixels from the top and the bottom, not 56,
    // so that its one frame, with the warning, fits the smallest window.
    let margin = if phrase_screen {
        Padding::from([sp::S24, sp::S56])
    } else {
        Padding::from(sp::S56)
    };
    row![
        brand(),
        container(
            scrollable(
                container(container(form).max_width(480.0))
                    .padding(margin)
                    .center_x(Length::Fill),
            )
            .height(Length::Shrink)
            .style(theme::scroll),
        )
        .center(Length::Fill)
        .width(Length::FillPortion(1)),
    ]
    .height(Length::Fill)
    .into()
}

/// The phone's frame: the wordmark stands
/// over "A wallet for Mochimo" where the brand panel was, and the form follows
/// it on one page. The page scrolls as a whole, between 16-pixel margins,
/// because a phone has no room for a second column.
fn phone_frame(form: Element<'_, Message>) -> Element<'_, Message> {
    let strip = column![
        t("TAWARA", ty::WORDMARK, color::TEXT_PRIMARY),
        t(
            "A wallet for Mochimo",
            Type {
                font: fonts::BODY_MEDIUM,
                size: 13.0
            },
            color::TEXT_SECONDARY
        ),
    ]
    .spacing(sp::S4);
    // The strip and the form share one column, so that they align at every
    // width below the compact limit. The 24 pixels between them are the form
    // block's top padding.
    let page = column![
        phone_block(strip, sp::S16, 0.0),
        phone_block(form, sp::S24, sp::S32),
    ];
    // The page takes the window's size in both directions, since a scrollable
    // is otherwise only as large as its content.
    container(
        scrollable(page)
            .id(crate::screens::page_id())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::scroll),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// A block of a phone's page: its content at most 480 wide and centred, with
/// 16 pixels of margin at each side and `top` and `bottom` above and below.
fn phone_block<'a>(
    content: impl Into<Element<'a, Message>>,
    top: f32,
    bottom: f32,
) -> Element<'a, Message> {
    container(container(content).max_width(480.0).width(Length::Fill))
        .padding(Padding {
            top,
            right: sp::S16,
            bottom,
            left: sp::S16,
        })
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
}

/// The brand panel: the green field, the application's own wordmark, the
/// headline and the feature pills.
fn brand<'a>() -> Element<'a, Message> {
    container(
        column![
            brand_wordmark(ty::WORDMARK_LARGE),
            space().height(Length::Fill),
            column![brand_headline(ty::HERO, ty::HERO_ITALIC), brand_pills()].spacing(sp::S24),
        ]
        .height(Length::Fill),
    )
    .padding(Padding::from([sp::S48, sp::S56]))
    .width(Length::FillPortion(1))
    .height(Length::Fill)
    .style(theme::brand_panel)
    .into()
}

/// The application's wordmark over "A wallet for Mochimo", on the brand
/// field, the name at `name`'s size.
fn brand_wordmark<'a>(name: Type) -> Element<'a, Message> {
    let ink = color::TEXT_ON_ACCENT;
    column![
        t("TAWARA", name, ink),
        t(
            "A wallet for Mochimo",
            Type {
                font: fonts::BODY_MEDIUM,
                size: 13.0
            },
            ink
        ),
    ]
    .spacing(sp::S2)
    .into()
}

/// The headline on the brand field: its first line in `upright`, its second
/// in `italic`. The desktop sets both at 52 points; a phone sets them
/// smaller ([`ty::HERO_PHONE`]).
fn brand_headline<'a>(upright: Type, italic: Type) -> Element<'a, Message> {
    let ink = color::TEXT_ON_ACCENT;
    column![
        t("YOUR CRYPTO JOURNEY", upright, ink)
            .line_height(LineHeight::Relative(1.05))
            .width(Length::Fill),
        t("STARTS WITH US.", italic, ink).line_height(LineHeight::Relative(1.05)),
    ]
    .into()
}

/// The three feature pills on the brand field, which wrap onto a line of
/// their own when the width runs out.
fn brand_pills<'a>() -> Element<'a, Message> {
    let pill = |label| ui::pill(label, ty::PILL, color::BG_APP, color::TEXT_PRIMARY, 36.0);
    row![
        pill("WOTS+ post-quantum signatures"),
        pill("Perpetual account tags"),
        pill("256 destinations per send"),
    ]
    .spacing(sp::S10)
    .wrap()
    .vertical_spacing(sp::S10)
    .into()
}

/// The welcome screen, on a phone alone, when the phone's own folder holds
/// no store. The brand field whole: the wordmark at its top, and the
/// headline, the pills and "Get started" at its foot, the button in thumb
/// reach. It does not scroll, and a test of this module checks that it fits
/// 360 by 640 with the field's 16-pixel margins. The field lies within the
/// window's safe area, which pads every screen's root (`screens::view`), and
/// the strips the safe area leaves are coloured as the field is, so the
/// green reaches the window's edges.
fn welcome<'a>() -> Element<'a, Message> {
    let page = column![
        brand_wordmark(ty::WORDMARK_LARGE),
        space().height(Length::Fill),
        column![
            brand_headline(ty::HERO_PHONE, ty::HERO_PHONE_ITALIC),
            brand_pills()
        ]
        .spacing(sp::S24),
        // The button is the dark tonal one, not the green primary: the field
        // is the primary's colour, so a primary button would not show on it.
        ui::button_with(
            "Get started",
            theme::Button::Tonal,
            Size::Xl,
            None,
            Some(Message::Go(Go::Start)),
        ),
    ]
    .spacing(sp::S40);
    // The field is the window's whole width and height; its margins are the
    // padding of the page inside it.
    container(
        container(page)
            .padding(Padding::from([sp::S24, sp::S16]))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::brand_panel)
    .into()
}

/// Where the person's keys are, as the first-run screens say it: "computer",
/// or "phone" in a phone shell.
fn place(model: &Model) -> &'static str {
    if model.phone() { "phone" } else { "computer" }
}

/// A title over an introduction.
fn heading<'a>(
    title: &str,
    intro: impl iced::widget::text::IntoFragment<'a>,
) -> Element<'a, Message> {
    column![
        t(
            title.to_uppercase(),
            ty::ONBOARDING_TITLE,
            color::TEXT_PRIMARY
        ),
        t(intro, ty::INTRO, color::TEXT_SECONDARY).line_height(LineHeight::Relative(1.55)),
    ]
    .spacing(sp::S8)
    .into()
}

/// The node footer: which node the wallet asks, and a way to change it. A
/// node not asked yet, as the default is until a wallet opens, is drawn muted
/// and says so, apart from one that answered. On a phone the URL takes a line
/// of its own, broken at its glyphs when it is long, so that a long address
/// cannot push the link off the edge.
fn node_footer(model: &Model, back: Back) -> Element<'_, Message> {
    let (dot, label, url) = match (&model.node.url, &model.node.tip, &model.node.error) {
        (Some(url), Some(_), None) => (color::ACCENT, "Mesh API", url.as_str()),
        (Some(url), None, None) => (color::TEXT_MUTED, "API Not Yet Connected", url.as_str()),
        (Some(url), _, Some(_)) => (color::WARNING, "Mesh API, not answering", url.as_str()),
        (None, ..) => (color::TEXT_MUTED, "No API chosen", ""),
    };
    let change = if model.node.url.is_some() {
        "Change API"
    } else {
        "Choose an API"
    };
    let link = ui::link(change, ty::LINK_SMALL, Message::Go(Go::Node(back)));
    let line: Element<'_, Message> = if model.compact() {
        let mut lines = column![
            row![
                ui::dot(dot, 8.0),
                t(label, ty::NOTE, color::TEXT_MUTED),
                space().width(Length::Fill),
                link,
            ]
            .spacing(sp::S8)
            .align_y(Alignment::Center),
        ]
        .spacing(sp::S6);
        // An empty URL, when no node is chosen, has no line to draw.
        if !url.is_empty() {
            lines = lines
                .push(t(url, ty::MONO_TINY, color::TEXT_SECONDARY).wrapping(Wrapping::WordOrGlyph));
        }
        lines.into()
    } else {
        row![
            ui::dot(dot, 8.0),
            t(label, ty::NOTE, color::TEXT_MUTED),
            t(url, ty::MONO_TINY, color::TEXT_SECONDARY),
            space().width(Length::Fill),
            link,
        ]
        .spacing(sp::S8)
        .align_y(Alignment::Center)
        .into()
    };
    column![ui::divider(), line].spacing(sp::S16).into()
}

/// The start screen: create or restore.
fn start(model: &Model, choice: StartChoice) -> Element<'_, Message> {
    let compact = model.compact();
    let mut body = column![
        heading(
            "Get started",
            format!(
                "Your keys are created and encrypted on this {}. Nothing secret ever leaves it.",
                place(model)
            ),
        ),
        column![
            radio_card(
                compact,
                choice == StartChoice::Create,
                "Create a new wallet",
                Some("Recommended"),
                "A fresh 24-word recovery phrase and your first account.",
                Message::Choose(StartChoice::Create),
            ),
            radio_card(
                compact,
                choice == StartChoice::Restore,
                "Restore from recovery phrase",
                None,
                format!(
                    "12 to 24 words you already have. The keystore is made from them on this {}; \
                     accounts already on the chain are found from the wallet after.",
                    place(model)
                ),
                Message::Choose(StartChoice::Restore),
            ),
        ]
        .spacing(sp::S12),
        ui::button_with(
            "Continue",
            theme::Button::Primary,
            Size::Xl,
            None,
            Some(Message::Continue),
        ),
    ]
    .spacing(sp::S24);
    // A phone has no link to a store here: the store it holds is the one the
    // unlock screen opens, and the phone is shown this screen only when it
    // holds none.
    if !model.phone() {
        body = body.push(ui::link(
            "Open a wallet already on this computer",
            ty::LINK,
            Message::Go(Go::Unlock),
        ));
    }
    body.push(node_footer(model, Back::Start)).into()
}

/// A radio card. It acts on release, not on press, so that a drag which
/// starts on it scrolls the page; a desktop click is a press and a release in
/// place, so nothing changes there. On a phone the card has 16 pixels of
/// padding, and its badge wraps under the title when the two do not fit one
/// line.
fn radio_card<'a>(
    compact: bool,
    selected: bool,
    title: &'a str,
    badge: Option<&'a str>,
    description: impl iced::widget::text::IntoFragment<'a>,
    on_press: Message,
) -> Element<'a, Message> {
    let mut title_row = row![t(title, ty::RADIO_TITLE, color::TEXT_PRIMARY)]
        .spacing(sp::S8)
        .align_y(Alignment::Center);
    if let Some(b) = badge {
        title_row = title_row.push(ui::pill(
            b,
            ty::BADGE,
            color::ACCENT,
            color::TEXT_ON_ACCENT,
            18.0,
        ));
    }
    let title_row: Element<'a, Message> = if compact {
        title_row.wrap().vertical_spacing(sp::S4).into()
    } else {
        title_row.into()
    };
    tap(container(
        row![
            radio_mark(selected),
            column![
                title_row,
                t(description, ty::RADIO_TEXT, color::TEXT_SECONDARY)
                    .line_height(LineHeight::Relative(1.5)),
            ]
            .spacing(sp::S4)
            .width(Length::Fill),
        ]
        .spacing(sp::S14)
        .align_y(Alignment::Start),
    )
    .padding(if compact { sp::S16 } else { sp::S18 })
    .width(Length::Fill)
    .style(theme::radio_card(selected)))
    .on_tap(on_press)
    .into()
}

/// The radio's mark: a white disc, or a green ring round a dark gap and a
/// green dot.
fn radio_mark<'a>(selected: bool) -> Element<'a, Message> {
    let disc = |size: f32, fill| {
        container(space())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .style(theme::pill(fill, fill))
    };
    let mark: Element<'a, Message> = if selected {
        container(
            container(disc(10.0, color::ACCENT))
                .center(Length::Fixed(16.0))
                .style(theme::pill(
                    iced::Color::from_rgb8(0x3B, 0x3B, 0x3B),
                    color::ACCENT,
                )),
        )
        .center(Length::Fixed(20.0))
        .style(theme::pill(color::ACCENT, color::ACCENT))
        .into()
    } else {
        disc(20.0, color::TEXT_PRIMARY).into()
    };
    container(mark).padding(Padding::from([sp::S2, 0.0])).into()
}

/// The node screen.
fn node<'a>(model: &'a Model, form: &'a NodeForm) -> Element<'a, Message> {
    let back = match form.back {
        Back::Unlock => Go::Unlock,
        Back::Wallet if model.wallet.is_some() => Go::Wallet,
        // A phone that holds a store is not offered the start screens, so its
        // way back leads to the unlock screen, as `App::go_back`'s does; a
        // link to the start screen there would do nothing.
        Back::Start | Back::Wallet if model.phone_holds_store() => Go::Unlock,
        Back::Start => Go::Start,
        Back::Wallet => Go::Wallet,
    };
    let mut links = row![ui::link("Back", ty::LINK, Message::Go(back))].spacing(sp::S24);
    // Forget is also the way out of a preferences file that could not be read,
    // since the wallet has no node until the person chooses one or forgets it.
    if model.node.url.is_some() || model.prefs_held {
        links = links.push(ui::link("Forget the API", ty::LINK, Message::ForgetNode));
    }
    // On a phone the links wrap onto a second line when the width runs out.
    let links: Element<'a, Message> = if model.compact() {
        links.wrap().vertical_spacing(sp::S8).into()
    } else {
        links.into()
    };
    // With no node in use, after Forget or while the preferences cannot be
    // read, "by default the wallet uses" would read as the node in use and
    // contradict the held note, so the text says none is, as Settings' node
    // card does.
    let using = if model.node.url.is_some() {
        format!("By default the wallet uses {DEFAULT_NODE}; you can choose another API, or none.")
    } else {
        format!("The wallet uses no API now; you can choose one. The default is {DEFAULT_NODE}.")
    };
    let mut body = column![
        heading(
            "Choose an API",
            format!(
                "{using} Every balance, key index and chain tip the wallet works from comes from \
                 the API it uses, so use one you trust. That API learns which tags the wallet \
                 asks about, and so their balances, and when the wallet is used; it can also \
                 learn your network address. Requests to {DEFAULT_NODE} pass through Cloudflare, \
                 which can read them and sees your network address. https:// is required; http:// \
                 is accepted only to this {}.",
                place(model)
            ),
        ),
        ui::field(
            "API URL",
            "https://",
            &form.url,
            ty::MONO,
            false,
            Message::NodeUrl,
            Some(Message::SaveNode),
        ),
    ]
    .spacing(sp::S24);
    if model.prefs_held {
        body = body.push(ui::warning_callout(HELD_TITLE, HELD_BODY));
    }
    if let Some(e) = &form.error {
        body = body.push(ui::refusal(e));
    } else if let Some(e) = &model.node.error {
        body = body.push(ui::refusal(e));
    }
    let save = (!form.url.trim().is_empty()).then_some(Message::SaveNode);
    body.push(ui::button_with(
        "Use this API",
        theme::Button::Primary,
        Size::Xl,
        None,
        save,
    ))
    .push(links)
    .into()
}

/// The folder and the two passwords, shared by the new-wallet and restore
/// screens. `drawn` is the fields' generation (`Typed`). The folder, and the
/// callout about a synced one, are drawn only when `folder` says the person
/// chooses it: a phone keeps every store in the app's own folder.
fn password_fields(
    form: &PasswordForm,
    drawn: u64,
    submit: Option<Message>,
    folder: bool,
) -> Element<'_, Message> {
    let mut fields = column![].spacing(sp::S16);
    if folder {
        let field = ui::field("Folder", "", &form.dir, ty::MONO, false, Message::Dir, None);
        // The desktop's folder dialog.
        fields = fields.push(if crate::folder::OFFERED {
            Element::from(
                row![
                    container(field).width(Length::Fill),
                    ui::button_with(
                        "Browse",
                        theme::Button::Secondary,
                        Size::Header,
                        None,
                        Some(Message::BrowseDir),
                    ),
                ]
                .spacing(sp::S12)
                .align_y(Alignment::End),
            )
        } else {
            field
        });
        if let Some(w) = &form.synced {
            fields = fields.push(ui::warning_callout("This folder is synced", w.as_str()));
        }
    }
    fields
        .push(
            column![
                ui::field(
                    "Password",
                    "",
                    &form.password,
                    ty::FIELD,
                    true,
                    move |text| Message::Password(Typed { text, drawn }),
                    None,
                ),
                ui::helper(format!("At least {MIN_PASSWORD_LEN} characters.")),
            ]
            .spacing(sp::S6),
        )
        .push(ui::field(
            "Password again",
            "",
            &form.again,
            ty::FIELD,
            true,
            move |text| Message::Again(Typed { text, drawn }),
            submit,
        ))
        .into()
}

/// Creating a wallet: the folder and the password for a new store.
fn new_wallet<'a>(model: &'a Model, form: &'a PasswordForm) -> Element<'a, Message> {
    let ready = !form.password.is_empty() && !form.again.is_empty() && !form.dir.trim().is_empty();
    let intro = if model.phone() {
        "Choose the password that seals the keystore. The keystore is kept in the app's own folder. \
         Nothing is written until you have confirmed the recovery phrase."
    } else {
        "The keystore is the encrypted file containing your wallet's accounts and signing keys. \
         Choose where to store this file on your local computer. The default location is \
         selected for you already."
    };
    let mut body = column![heading("New wallet", intro),].spacing(sp::S24);
    if let Some(note) = &form.note {
        body = body.push(ui::accent_callout(
            crate::icon::Icon::Lock,
            "Nothing was created",
            note.as_str(),
        ));
    }
    body = body.push(password_fields(
        form,
        model.drawn(),
        ready.then_some(Message::CreateWallet),
        !model.phone(),
    ));
    if let Some(e) = &form.error {
        body = body.push(ui::refusal(e));
    }
    body.push(ui::button_with(
        "Continue",
        theme::Button::Primary,
        Size::Xl,
        None,
        ready.then_some(Message::CreateWallet),
    ))
    .push(ui::link("Back", ty::LINK, Message::Go(Go::Start)))
    .push(node_footer(model, Back::Start))
    .into()
}

/// The warning over a new phrase: the library's title (`cli::create`), with
/// the application's own sentence in place of the library's.
const PHRASE_TITLE: &str = "WRITE THIS DOWN. It is shown once.";
const PHRASE_BODY: &str = "This is the only way you can recover your wallet if you forget the \
                           password, or lose access to this computer, so write it down now, and \
                           in the correct order.";

/// The warning after a screenshot taken while the words were drawn, in the
/// style of the library's warning. On a phone it takes the words' place until
/// "Show the phrase" is pressed.
const PHRASE_SHOT_TITLE: &str = "A screenshot was taken";
const PHRASE_SHOT_BODY: &str = "Delete it. It holds the words.";

/// What stands where the words are while the screen is recorded or mirrored.
/// It says why, and that iOS does not report every recording or mirror, since
/// the blanking is best effort.
const PHRASE_HIDDEN: &str = "While iOS reports this screen as recorded or mirrored, the words are \
                             hidden, and they come back when it stops. iOS does not report every \
                             recording or mirror, so the words may show during one.";

/// The height of the words' grid at its 14-pixel words: eight rows of cells,
/// with [`PHRASE_GAP`] between the rows. The placeholder that replaces the grid
/// while the screen is captured, and the warning that takes its place after a
/// screenshot on a phone, are as tall, so the page does not move.
/// `the_placeholder_is_as_tall_as_the_grid_it_replaces` holds this to the
/// layout.
const PHRASE_GRID_HEIGHT: f32 = 251.6;

/// The columns the words are read down: three, on a phone and on the desktop
/// alike.
const PHRASE_COLUMNS: usize = 3;

/// The gap between the cells of the words, across and down.
const PHRASE_GAP: f32 = sp::S6;

/// The usable height, in logical pixels, at which a phone draws the warning
/// and the words on one frame: the frame's own height at 360 wide, where the
/// heading and the warning wrap onto the most lines (732.4 pixels, rounded
/// up). A shorter phone draws two frames; a 360 by 800 phone has 719 pixels
/// between its insets, so it is shorter.
const PHRASE_ONE_FRAME: f32 = 733.0;

/// What the phrase screen draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhraseFrame {
    /// The warning and the words together: the desktop, and a phone tall
    /// enough for both.
    Whole,
    /// A phone shorter than [`PHRASE_ONE_FRAME`], before "Show the phrase": the
    /// library's warning, on its own.
    Warning,
    /// A phone shorter than [`PHRASE_ONE_FRAME`], after "Show the phrase" or a
    /// screenshot: the words, or the warning in their place, with their
    /// checkbox, Continue and the link.
    Phrase,
}

/// What the phrase screen draws of a screenshot taken while its words were
/// drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shot {
    /// No warning: no screenshot was taken, or the words are drawn after one.
    Clear,
    /// The desktop, which no phone shell confines and no shell reports a
    /// screenshot for: the warning comes above the words.
    Above,
    /// A phone, at any width and held sideways too: the words are hidden, and
    /// the warning takes their place, with "Show the phrase", until the person
    /// presses it.
    InPlace,
}

/// The frame a phone whose page is `height` pixels tall draws for a phrase,
/// given whether the person has pressed "Show the phrase" (`shown`) and whether
/// a screenshot was taken while the words were drawn (`photographed`). A
/// screenshot hides the words, not the frame: a short phone keeps the frame it
/// had after "Show the phrase".
fn phrase_frame(height: f32, shown: bool, photographed: bool) -> PhraseFrame {
    if height >= PHRASE_ONE_FRAME {
        PhraseFrame::Whole
    } else if shown || photographed {
        PhraseFrame::Phrase
    } else {
        PhraseFrame::Warning
    }
}

/// Whether a phone hides the words of its phrase behind the warning after a
/// screenshot taken while they were drawn, until "Show the phrase" is pressed
/// again.
fn words_hidden_by_shot(p: &PhraseState) -> bool {
    p.photographed && !p.shown
}

/// What the desktop's frame draws of a screenshot taken while its words were
/// drawn. A phone shell's screenshots
/// come in whatever its orientation, so a phone held sideways, which draws
/// that frame, puts the warning in the words' place as a tall phone does, at
/// any width. Only a desktop, which no phone shell confines, keeps the warning
/// above the words.
fn wide_shot(model: &Model, p: &PhraseState) -> Shot {
    if model.phone() {
        if words_hidden_by_shot(p) {
            Shot::InPlace
        } else {
            Shot::Clear
        }
    } else if p.photographed {
        Shot::Above
    } else {
        Shot::Clear
    }
}

/// Whether the phrase screen draws its words now: on the
/// desktop, always; on a phone, when it is too short for the warning and the
/// words on one frame, only once "Show the phrase" is pressed; and on a taller
/// phone, unless a screenshot has hidden them. A phone held sideways draws the
/// desktop's frame, and a screenshot hides its words there too, so they are
/// not noted again while they wait. It is the frame [`phone_phrase`] draws,
/// decided by the window's height as the model holds it, so that a screenshot
/// is noted only when the words were on screen.
pub(crate) fn phrase_words_drawn(model: &Model, p: &PhraseState) -> bool {
    if model.phone() && words_hidden_by_shot(p) {
        return false;
    }
    !model.compact()
        || (!words_hidden_by_shot(p)
            && phrase_frame(super::page_height(model), p.shown, p.photographed)
                != PhraseFrame::Warning)
}

/// The phrase screen on a phone, for the `words` of `p`: the frame its page
/// of `height` pixels allows, on a page of its own. The phone's wordmark is
/// left out, which is what lets the warning and the words share a frame on a
/// 390 by 844 phone.
fn phone_phrase<'a>(
    words: &[&'a str],
    p: &PhraseState,
    height: f32,
    captured: bool,
) -> Element<'a, Message> {
    let shot = if words_hidden_by_shot(p) {
        Shot::InPlace
    } else {
        Shot::Clear
    };
    let page = phone_block(
        phrase_content(
            words,
            p.written,
            phrase_frame(height, p.shown, p.photographed),
            shot,
            captured,
        ),
        sp::S24,
        sp::S32,
    );
    container(
        scrollable(page)
            .id(crate::screens::page_id())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::scroll),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// The phrase screen: a heading, the library's warning, and the 24 words in a
/// grid with the checkbox, Continue and a link. The warning comes before the
/// words. `frame` decides what is drawn: both together, or the warning with
/// "Show the phrase", or the words alone. A screenshot taken while the words
/// were drawn is drawn as `shot` says: above the words on the desktop, or in
/// their place on a phone, held sideways too, until "Show the phrase" is
/// pressed. While the screen is recorded or mirrored (`captured`) the grid is
/// a placeholder of its height, whatever `shot` says.
fn phrase_content<'a>(
    words: &[&'a str],
    written: bool,
    frame: PhraseFrame,
    shot: Shot,
    captured: bool,
) -> Element<'a, Message> {
    let mut body = column![t(
        "YOUR RECOVERY PHRASE",
        ty::ONBOARDING_TITLE,
        color::TEXT_PRIMARY
    )]
    .spacing(sp::S20);
    if frame != PhraseFrame::Phrase {
        body = body.push(ui::warning_callout(PHRASE_TITLE, PHRASE_BODY));
    }
    if shot == Shot::Above {
        body = body.push(ui::warning_callout(PHRASE_SHOT_TITLE, PHRASE_SHOT_BODY));
    }
    if frame == PhraseFrame::Warning {
        return body.push(show_the_phrase()).into();
    }
    let grid = if captured {
        phrase_hidden()
    } else if shot == Shot::InPlace {
        phrase_shot()
    } else {
        words_grid(words)
    };
    body.push(grid)
        .push(
            checkbox(written)
                .label("I have written down every word, in order.")
                .font(fonts::BODY)
                .text_size(13)
                .on_toggle(Message::Written),
        )
        .push(ui::button_with(
            "Continue",
            theme::Button::Primary,
            Size::Xl,
            None,
            written.then_some(Message::ToConfirm),
        ))
        .push(ui::link(
            "Start over with a new phrase",
            ty::LINK,
            Message::StartOver,
        ))
        .into()
}

/// The words in a grid of [`PHRASE_COLUMNS`] columns, read down each column,
/// with [`PHRASE_GAP`] between the cells.
fn words_grid<'a>(words: &[&'a str]) -> Element<'a, Message> {
    let mut grid = column![].spacing(PHRASE_GAP);
    for cells in phrase_grid(words.len(), PHRASE_COLUMNS) {
        let mut line = row![].spacing(PHRASE_GAP);
        for cell in cells {
            let word = cell.and_then(|n| words.get(n).map(|w| (n, *w)));
            let cell: Element<'a, Message> = match word {
                Some((n, w)) => phrase_cell(n, w),
                None => space().width(Length::FillPortion(1)).into(),
            };
            line = line.push(cell);
        }
        grid = grid.push(line);
    }
    grid.into()
}

/// What stands where the words are while the screen is recorded or mirrored: a
/// box as tall as the grid, with the reason in it. The box is the grid's own
/// field style, so the page keeps its shape.
fn phrase_hidden<'a>() -> Element<'a, Message> {
    container(
        t(PHRASE_HIDDEN, ty::CALLOUT_BODY, color::TEXT_SECONDARY)
            .line_height(LineHeight::Relative(1.5))
            .width(Length::Fill)
            .align_x(Alignment::Center),
    )
    .padding(sp::S16)
    .width(Length::Fill)
    .center_y(Length::Fixed(PHRASE_GRID_HEIGHT))
    .style(theme::field_box)
    .into()
}

/// What stands where the words are after a screenshot taken while they were
/// drawn, on a phone: the warning, and "Show the phrase" under it, in a box as
/// tall as the grid, so that the page does not move.
fn phrase_shot<'a>() -> Element<'a, Message> {
    container(
        column![
            ui::warning_callout(PHRASE_SHOT_TITLE, PHRASE_SHOT_BODY),
            show_the_phrase(),
        ]
        .spacing(sp::S16)
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .center_y(Length::Fixed(PHRASE_GRID_HEIGHT))
    .into()
}

/// "Show the phrase": on a short phone, the words come back after the library's
/// warning; on any phone, they come back after a screenshot.
fn show_the_phrase<'a>() -> Element<'a, Message> {
    ui::button_with(
        "Show the phrase",
        theme::Button::Primary,
        Size::Xl,
        None,
        Some(Message::ShowPhrase),
    )
    .into()
}

/// One word of the phrase with its number: the number at 11 pixels, the word at
/// [`ty::PHRASE_WORD`], in a box with 8 pixels either side and 4 above and below.
/// At 360 wide a cell is about 105 pixels, and the widest word of the library's
/// list, "abstract", needs 67 at 14 and the number 13, with 6 between them: the
/// box leaves about 3 pixels spare, which the test
/// `the_widest_word_fits_its_cell_on_a_phone_of_360` holds to.
fn phrase_cell<'a>(n: usize, word: &'a str) -> Element<'a, Message> {
    container(
        row![
            t(format!("{:>2}", n + 1), ty::MONO_TINY, color::TEXT_MUTED),
            t(word, ty::PHRASE_WORD, color::TEXT_PRIMARY),
        ]
        .spacing(sp::S6)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([sp::S4, sp::S8]))
    .width(Length::FillPortion(1))
    .style(theme::field_box)
    .into()
}

/// Where the `len` words of a phrase go in a grid of `columns` columns: one
/// entry per row, holding the zero-based word each cell shows, or `None` for
/// a cell with no word. The phrase is read down the first column and then the
/// next, so with three columns words 1 to 8 fill the first, 9 to 16 the second
/// and 17 to 24 the third.
fn phrase_grid(len: usize, columns: usize) -> Vec<Vec<Option<usize>>> {
    let rows = len.div_ceil(columns);
    (0..rows)
        .map(|r| {
            (0..columns)
                .map(|c| {
                    let n = c * rows + r;
                    (n < len).then_some(n)
                })
                .collect()
        })
        .collect()
}

/// Confirming the phrase: three words, to show it was written down.
fn confirm(p: &PhraseState, drawn: u64) -> Element<'_, Message> {
    let [a, b, c] = p.positions;
    let ready = p.words.iter().all(|w| !w.trim().is_empty());
    let mut fields = column![].spacing(sp::S16);
    for (i, position) in p.positions.iter().enumerate() {
        let last = i == 2;
        fields = fields.push(ui::field(
            format!("Word {position}"),
            "",
            &p.words[i],
            ty::FIELD,
            true,
            move |text| Message::Word(i, Typed { text, drawn }),
            (last && ready).then_some(Message::ConfirmWords),
        ));
    }
    let mut body =
        column![
            column![
            t("CONFIRM YOUR PHRASE", ty::ONBOARDING_TITLE, color::TEXT_PRIMARY),
            t(
                format!(
                    "Type words {a}, {b} and {c} of the phrase, as you wrote them down. Nothing \
                     is written until they match."
                ),
                ty::INTRO,
                color::TEXT_SECONDARY,
            )
            .line_height(LineHeight::Relative(1.55)),
        ]
            .spacing(sp::S8),
            fields,
        ]
        .spacing(sp::S24);
    if let Some(e) = &p.error {
        body = body.push(ui::refusal(e));
    }
    body.push(ui::button_with(
        "Create the wallet",
        theme::Button::Primary,
        Size::Xl,
        None,
        ready.then_some(Message::ConfirmWords),
    ))
    .push(ui::link(
        "Show the phrase again",
        ty::LINK,
        Message::BackToPhrase,
    ))
    .into()
}

/// Which recovery phrases Restore reaches funds with, shown before the phrase
/// is typed. A phrase from any other scheme is accepted without an error and
/// derives a different wallet.
const PHRASE_COMPATIBILITY: &str = "Tawara is compatible with Mochimo seed phrases created by \
     Tawara, the Mochimo Chrome Extension, and the Mochimo Android wallet app. Seed from other \
     Mochimo wallet apps will not work, but will instead produce a new wallet with different \
     accounts.";

fn scheme_warning<'a>() -> Element<'a, Message> {
    ui::warning_note(PHRASE_COMPATIBILITY)
}

/// Restoring: a store from a phrase the person already has.
fn restore<'a>(model: &Model, r: &'a RestoreForm, drawn: u64) -> Element<'a, Message> {
    let ready = !r.form.password.is_empty()
        && !r.form.again.is_empty()
        && !r.phrase.trim().is_empty()
        && !r.form.dir.trim().is_empty();
    let mut body = column![
        t(
            "RESTORE FROM A PHRASE",
            ty::ONBOARDING_TITLE,
            color::TEXT_PRIMARY,
        ),
        scheme_warning(),
        password_fields(&r.form, drawn, None, !model.phone()),
        phrase_field(model, r, drawn, ready),
    ]
    .spacing(sp::S20);
    if let Some(e) = &r.form.error {
        body = body.push(ui::refusal(e));
    }
    body.push(ui::button_with(
        "Restore",
        theme::Button::Primary,
        Size::Xl,
        None,
        ready.then_some(Message::RestoreWallet),
    ))
    .push(ui::link("Back", ty::LINK, Message::Go(Go::Start)))
    .into()
}

/// The restore screen's recovery-phrase field, drawn as `ui::field` draws a
/// field, with what keyboard paste brings cleaned as a phrase, and on a phone,
/// at any width, a Paste button beside it. The button's read carries the
/// generation the field is drawn at, as the field's own text does.
fn phrase_field<'a>(
    model: &Model,
    r: &'a RestoreForm,
    drawn: u64,
    ready: bool,
) -> Element<'a, Message> {
    let field = text_input("", &r.phrase)
        .id(super::phrase_id())
        .font(ty::FIELD.font)
        .size(ty::FIELD.size)
        .padding(Padding::from([sp::S12, sp::S14]))
        .secure(true)
        .style(theme::field(radius::R12))
        .on_input(move |text| Message::PhraseText(Typed { text, drawn }))
        .on_submit_maybe(ready.then_some(Message::RestoreWallet));
    let entry = if model.phone() {
        row![
            paste::phrase_paste(field),
            paste::button(Some(Message::Paste(PasteInto::Phrase { drawn }))),
        ]
        .spacing(sp::S8)
        .align_y(Alignment::Center)
        .into()
    } else {
        paste::phrase_paste(field)
    };
    column![
        t("Recovery phrase", ty::FORM_LABEL, color::TEXT_SECONDARY),
        entry
    ]
    .spacing(sp::S6)
    .into()
}

/// Unlocking the store on this computer.
fn unlock<'a>(model: &'a Model, form: &'a UnlockForm) -> Element<'a, Message> {
    let ready = !form.password.is_empty() && !form.dir.trim().is_empty();
    let drawn = model.drawn();
    let mut body = column![heading(
        "Unlock",
        format!(
            "Open the keystore on this {} with its password. It stays open until you lock it, or \
             until nothing has been done for the auto-lock period.",
            place(model)
        ),
    )]
    .spacing(sp::S24);
    if let Some(note) = &form.note {
        body = body.push(ui::accent_callout(
            crate::icon::Icon::Lock,
            "The wallet locked",
            note.as_str(),
        ));
    }
    if !model.phone() {
        body = body.push(ui::field(
            "Folder",
            "",
            &form.dir,
            ty::MONO,
            false,
            Message::Dir,
            None,
        ));
    }
    // Where a shell offers the device unlock, its button stands above the
    // password field while the unlock is on, and the field stays as it is.
    // What the device unlock last found stands under the button, or in its
    // place while the unlock is off, so that the form says why.
    if let Some(device) = model.device.as_ref() {
        if model.device_unlock_on() {
            body = body.push(ui::button_with(
                format!("Unlock with {}", device.check().name()),
                theme::Button::Secondary,
                Size::Xl,
                Some(crate::icon::Icon::Lock),
                Some(Message::UnlockWithDevice),
            ));
        }
        if let Some(note) = &model.device_note {
            body = body.push(ui::refusal(note.as_str()));
        }
    }
    body = body.push(ui::field(
        "Password",
        "",
        &form.password,
        ty::FIELD,
        true,
        move |text| Message::Password(Typed { text, drawn }),
        ready.then_some(Message::UnlockWallet),
    ));
    if let Some(e) = &form.error {
        body = body.push(ui::refusal(e));
    }
    body = body.push(ui::button_with(
        "Unlock",
        theme::Button::Primary,
        Size::Xl,
        None,
        ready.then_some(Message::UnlockWallet),
    ));
    // On a phone there is no link to a second wallet. The phone's one store is
    // the one this screen opens, and a new wallet in its folder would be
    // refused only after its password had been typed.
    if !model.phone() {
        body = body.push(ui::link(
            "Create or restore a different wallet",
            ty::LINK,
            Message::Go(Go::Start),
        ));
    }
    body.push(node_footer(model, Back::Unlock)).into()
}

/// The progress screen: the command a store is being made or opened by,
/// waiting its turn or under way, what the worker is doing for it, and a way
/// to stop it.
fn opening(busy: &Busy) -> Element<'_, Message> {
    let (title, what) = super::activity(busy);
    let mut body = column![heading(title, what)].spacing(sp::S20);
    if let Some(detail) = super::progress(busy) {
        body = body.push(detail);
    }
    body.push(ui::button_with(
        "Cancel",
        theme::Button::Secondary,
        Size::Medium,
        None,
        Some(Message::Cancel),
    ))
    .into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use iced::advanced::layout;
    use iced::advanced::widget::Tree;
    use iced::advanced::widget::operation::focusable;
    use iced::mobile::SafeArea;
    use iced::{Point, Rectangle, Size};
    use tawara_wallet_core::preferences::{DeviceUnlock, Preferences};
    use tawara_wallet_core::{Activity, CONFIRM_POSITIONS, PhraseForDisplay, Progress, RequestId};

    use super::*;
    use crate::app::{DEVICE_MISSING_NOTE, DEVICE_WRONG_NOTE, NARROWEST};
    use crate::harness::{
        Driver, finger_lift, finger_move, finger_press, fits, fits_at, fits_the_viewport,
        mouse_move, mouse_press, mouse_release, texts,
    };
    use crate::paste::support::{Holding, paste_keys};

    /// The phone's own folder, as the shells report it. The store is kept in a
    /// `keystore` folder inside it.
    const PHONE_DIR: &str = "/private/var/mobile/Containers/Data/Application/0A1B2C3D/Library/Application Support/Tawara";

    /// The node the screens show.
    const NODE: &str = "https://node.example";

    /// A phone of 360 logical pixels drawing `screen`, with its store in the
    /// phone's folder and a node chosen.
    fn phone_model(screen: Screen) -> Model {
        let mut model = Model::new(Preferences::default(), None);
        model.default_dir = Some(PathBuf::from(PHONE_DIR).join("keystore"));
        model.confine(PathBuf::from(PHONE_DIR));
        model.width = NARROWEST;
        model.node.url = Some(NODE.to_owned());
        model.screen = screen;
        model
    }

    /// Every first-run screen drawn on a phone, each with a name to report it
    /// by. The start screen is also drawn with the restore card chosen, the
    /// unlock screen with a note and with a refusal, and the progress screen
    /// at each of its three stages.
    fn phone_screens() -> Vec<(&'static str, Model)> {
        let folder = PathBuf::from(PHONE_DIR)
            .join("keystore")
            .display()
            .to_string();
        let password = "correct horse battery staple";
        let form = || PasswordForm {
            dir: folder.clone(),
            password: password.to_owned(),
            again: password.to_owned(),
            ..PasswordForm::default()
        };
        let phrase = || PhraseState {
            phrase: PhraseForDisplay::example(),
            dir: folder.clone(),
            positions: CONFIRM_POSITIONS,
            written: true,
            words: Default::default(),
            error: None,
            shown: false,
            photographed: false,
        };
        let busy = |activity, progress| Busy {
            id: RequestId::example(),
            activity,
            progress,
        };
        let mut waiting = phone_model(Screen::Unlock(UnlockForm::default()));
        waiting.busy = Some(busy(None, None));
        let mut opening = phone_model(Screen::Unlock(UnlockForm::default()));
        opening.busy = Some(busy(Some(Activity::DerivingKey), None));
        let mut reconciling = phone_model(Screen::Unlock(UnlockForm::default()));
        reconciling.busy = Some(busy(
            Some(Activity::AskingNode),
            Some(Progress {
                account: 2,
                accounts: 3,
                position: 3_840,
                ceiling: 10_017,
            }),
        ));
        vec![
            ("S0", phone_model(Screen::Welcome)),
            (
                "S1",
                phone_model(Screen::Start {
                    choice: StartChoice::Create,
                }),
            ),
            (
                "S1 with restore chosen",
                phone_model(Screen::Start {
                    choice: StartChoice::Restore,
                }),
            ),
            (
                "S2",
                phone_model(Screen::Node(NodeForm {
                    url: NODE.to_owned(),
                    error: None,
                    back: Back::Start,
                })),
            ),
            ("S2 with the preferences held", {
                let mut model = phone_model(Screen::Node(NodeForm {
                    url: String::new(),
                    error: None,
                    back: Back::Start,
                }));
                model.node.url = None;
                model.prefs_held = true;
                model
            }),
            ("S3", phone_model(Screen::NewWallet(form()))),
            (
                "S3 with a discarded phrase",
                phone_model(Screen::NewWallet(PasswordForm {
                    note: Some(
                        "The recovery phrase was discarded after 5 minutes with nothing done, \
                         before it was confirmed, and no store was written. Choose the password \
                         again and continue to get a new phrase."
                            .to_owned(),
                    ),
                    ..form()
                })),
            ),
            ("S4", phone_model(Screen::Phrase(phrase()))),
            (
                "S5",
                phone_model(Screen::Confirm(PhraseState {
                    words: ["abandon".into(), "abandon".into(), String::new()],
                    ..phrase()
                })),
            ),
            (
                "S6",
                phone_model(Screen::Restore(RestoreForm {
                    form: form(),
                    phrase: "abandon abandon abandon".to_owned(),
                })),
            ),
            (
                "S7",
                phone_model(Screen::Unlock(UnlockForm {
                    dir: folder.clone(),
                    ..UnlockForm::default()
                })),
            ),
            (
                "S7 with a note",
                phone_model(Screen::Unlock(UnlockForm {
                    dir: folder.clone(),
                    note: Some(
                        "Locked after 5 minutes with nothing done. The store is closed and its \
                         key is gone from memory; unlock it to go on."
                            .to_owned(),
                    ),
                    ..UnlockForm::default()
                })),
            ),
            (
                "S7 with a refusal",
                phone_model(Screen::Unlock(UnlockForm {
                    dir: folder.clone(),
                    error: Some("the password does not open the store in this folder".to_owned()),
                    ..UnlockForm::default()
                })),
            ),
            ("S8 waiting", waiting),
            ("S8 opening", opening),
            ("S8 reconciling", reconciling),
        ]
    }

    /// The first-run screen called `name`, drawn on a phone.
    fn named(name: &str) -> Model {
        phone_screens()
            .into_iter()
            .find(|(n, _)| *n == name)
            .map(|(_, model)| model)
            .unwrap_or_else(|| panic!("no first-run screen called {name}"))
    }

    /// The absolute bounds of every node `root` lays out to in a window of
    /// `width` by `height`, the root first. A node's position is its parent's
    /// plus its own, so the bounds are those drawn.
    fn drawn_bounds(root: Element<'_, Message>, width: f32, height: f32) -> Vec<Rectangle> {
        let mut element = root;
        let renderer = crate::harness::renderer();
        let mut tree = Tree::new(element.as_widget());
        let limits = layout::Limits::new(Size::ZERO, Size::new(width, height));
        let node = element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        let mut out = vec![node.bounds()];
        collect(&node, Point::ORIGIN, &mut out);
        out
    }

    /// Appends the bounds of every node under `node`, which sits at `origin`.
    fn collect(node: &layout::Node, origin: Point, out: &mut Vec<Rectangle>) {
        for child in node.children() {
            let bounds = child.bounds();
            let at = Point::new(origin.x + bounds.x, origin.y + bounds.y);
            out.push(Rectangle::new(at, bounds.size()));
            collect(child, at, out);
        }
    }

    /// The top and the bottom of the rows of the window at `x`, between `rows`
    /// pixels from the top, on which a finger that goes down and comes up in
    /// place publishes a message `wanted` accepts: the extent of the control
    /// it taps.
    fn tapped_rows(
        driver: &mut Driver<'_, Message>,
        x: f32,
        rows: std::ops::Range<u32>,
        wanted: impl Fn(&Message) -> bool,
    ) -> Option<(f32, f32)> {
        let mut span: Option<(f32, f32)> = None;
        for y in rows.step_by(2) {
            let y = y as f32;
            let (messages, _, _) = driver.send(&[finger_press(1, x, y), finger_lift(1, x, y)]);
            if messages.iter().any(&wanted) {
                span = Some(span.map_or((y, y), |(top, _)| (top, y)));
            }
        }
        span
    }

    #[test]
    fn every_first_run_screen_fits_a_phone_of_360() {
        let mut past = Vec::new();
        for (name, model) in phone_screens() {
            if let Err(boxes) = fits(view(&model), NARROWEST) {
                past.push(format!("{name}: {boxes:?}"));
            }
        }
        assert!(
            past.is_empty(),
            "nodes past 360 pixels:\n{}",
            past.join("\n")
        );
    }

    #[test]
    fn a_phone_page_is_one_column_across_the_window() {
        // The desktop puts the brand panel beside the form, two nodes at the top
        // level, each half the width. A phone has the page alone, the width of
        // the window.
        for (name, model) in phone_screens() {
            let mut element = view(&model);
            let renderer = crate::harness::renderer();
            let mut tree = Tree::new(element.as_widget());
            let limits = layout::Limits::new(Size::ZERO, Size::new(NARROWEST, 844.0));
            let root = element
                .as_widget_mut()
                .layout(&mut tree, &renderer, &limits);
            let page = root.children();
            assert_eq!(page.len(), 1, "{name}: one page, with no panel beside it");
            assert_eq!(
                page[0].bounds().width,
                NARROWEST,
                "{name}: the page spans the window"
            );
        }
    }

    /// Whether a window of `size` draws a text that reads `text`.
    fn shows(model: &Model, size: (f32, f32), text: &str) -> bool {
        texts(view(model), size)
            .iter()
            .any(|(drawn, _)| drawn == text)
    }

    #[test]
    fn the_welcome_screen_fits_a_phone_of_360_by_640_without_scrolling() {
        // The welcome screen is the brand field whole, with no page to scroll.
        // Nothing it lays out reaches past the window at 360 by 640, with no
        // safe area. iced fits each child into the room
        // its parent gives it, so an overflow shows as a squeezed child rather
        // than a child past the edge: the button's own size and place are the
        // check. Its 56 pixels and its 328 are kept, and its foot is the page's
        // bottom margin, which holds only if everything above it fits, since
        // the room between the wordmark and the headline takes what is left.
        let model = named("S0");
        let bounds = drawn_bounds(view(&model), NARROWEST, 640.0);
        let past: Vec<Rectangle> = bounds
            .iter()
            .copied()
            .filter(|r| {
                r.x < -0.5
                    || r.y < -0.5
                    || r.x + r.width > NARROWEST + 0.5
                    || r.y + r.height > 640.0 + 0.5
            })
            .collect();
        assert!(past.is_empty(), "nodes past 360 by 640: {past:?}");
        let button = bounds
            .iter()
            .copied()
            .find(|r| (r.width - (NARROWEST - 32.0)).abs() < 0.5 && (r.height - 56.0).abs() < 0.5)
            .expect("the button, at its full size");
        assert!(
            (button.y + button.height - (640.0 - 24.0)).abs() < 0.5,
            "the button does not stand at the foot: {button:?}"
        );
        let (_, label) = texts(view(&model), (NARROWEST, 640.0))
            .into_iter()
            .find(|(text, _)| text == "Get started")
            .expect("the button's label");
        assert!(
            label.center_y() > button.y && label.center_y() < button.y + button.height,
            "the label is not in the button: {label:?} in {button:?}"
        );
    }

    #[test]
    fn the_welcome_headline_sets_its_second_line_on_one_line_at_360() {
        // Its height is one line's, not two: a second
        // line would double it. The line is "STARTS WITH US.", the widest of
        // the headline, at the phone's 360 with its 16-pixel margins.
        let model = named("S0");
        let (_, line) = texts(view(&model), (NARROWEST, 640.0))
            .into_iter()
            .find(|(text, _)| text == "STARTS WITH US.")
            .expect("the headline's second line");
        assert!(
            line.height < ty::HERO_PHONE.size * 1.5,
            "two lines, not one: {line:?}"
        );
    }

    #[test]
    fn the_welcome_screen_draws_the_brand_field_and_get_started_leads_to_s1() {
        // The parts of the brand panel, and the one button, whose tap leads
        // to the start screen.
        let model = named("S0");
        let size = (NARROWEST, 640.0);
        for part in [
            "TAWARA",
            "A wallet for Mochimo",
            "YOUR CRYPTO JOURNEY",
            "STARTS WITH US.",
            "WOTS+ post-quantum signatures",
            "Perpetual account tags",
            "256 destinations per send",
            "Get started",
        ] {
            assert!(shows(&model, size, part), "{part} is drawn");
        }
        assert!(
            !shows(&model, size, "Continue"),
            "the welcome screen has no Continue"
        );
        let (_, at) = texts(view(&model), size)
            .into_iter()
            .find(|(text, _)| text == "Get started")
            .expect("the button's label");
        let mut driver = Driver::new(view(&model), (360, 640));
        let (messages, _, _) = driver.send(&[
            mouse_move(at.center_x(), at.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(
            matches!(messages.as_slice(), [Message::Go(Go::Start)]),
            "{messages:?}"
        );
    }

    #[test]
    fn s1_on_a_phone_offers_no_link_to_a_store_and_the_desktop_keeps_its_link() {
        // A phone has no store to open from the start screen: the one it holds
        // is the unlock screen's, and the start screen is shown only when it
        // holds none. The desktop keeps its link.
        let phone = named("S1");
        let size = (NARROWEST, 844.0);
        assert!(!shows(&phone, size, "Open the wallet on this phone"));
        assert!(!shows(
            &phone,
            size,
            "Open a wallet already on this computer"
        ));
        let mut desktop = Model::new(Preferences::default(), None);
        desktop.screen = Screen::Start {
            choice: StartChoice::Create,
        };
        assert!(shows(
            &desktop,
            (1440.0, 900.0),
            "Open a wallet already on this computer"
        ));
    }

    #[test]
    fn s7_on_a_phone_offers_no_link_to_a_second_wallet_and_the_desktop_keeps_its_link() {
        // The phone's one store is the one the unlock screen opens, and a
        // second wallet in its folder would be refused only after its password
        // was typed. The desktop keeps the link.
        let phone = named("S7");
        let size = (NARROWEST, 844.0);
        assert!(!shows(&phone, size, "Create or restore a different wallet"));
        assert!(shows(&phone, size, "Unlock"), "the unlock button is drawn");
        let mut desktop = Model::new(Preferences::default(), None);
        desktop.screen = Screen::Unlock(UnlockForm::default());
        assert!(shows(
            &desktop,
            (1440.0, 900.0),
            "Create or restore a different wallet"
        ));
    }

    /// The unlock screen on a phone whose shell offers Face ID, with the device
    /// unlock in `state` and `note` under its button.
    fn s7_with_device(state: DeviceUnlock, note: Option<&str>) -> Model {
        let mut model = named("S7");
        model.device = Some(crate::harness::face_id_device());
        model.prefs.device_unlock = state;
        model.device_note = note.map(str::to_owned);
        model
    }

    #[test]
    fn s7_draws_the_device_button_only_while_the_device_unlock_is_on() {
        // Off, not yet offered, or a shell with no device unlock: no button,
        // and the password form is unchanged.
        let size = (NARROWEST, 844.0);
        let button = "Unlock with Face ID";
        assert!(shows(&s7_with_device(DeviceUnlock::On, None), size, button));
        assert!(!shows(
            &s7_with_device(DeviceUnlock::Off, None),
            size,
            button
        ));
        assert!(!shows(
            &s7_with_device(DeviceUnlock::Unoffered, None),
            size,
            button
        ));
        assert!(!shows(&named("S7"), size, button), "no shell offers it");
        let mut desktop = Model::new(Preferences::default(), None);
        desktop.screen = Screen::Unlock(UnlockForm::default());
        assert!(!shows(&desktop, (1440.0, 900.0), button), "the desktop");
        assert!(
            shows(&named("S7"), size, "Unlock"),
            "the password form stays"
        );
    }

    #[test]
    fn s7_says_why_the_device_unlock_is_off_where_the_button_would_be() {
        // The unlock turns itself off when the kept password is missing or no
        // longer opens the store, and the note that says so stands above the
        // field, with no button.
        let size = (NARROWEST, 844.0);
        for note in [DEVICE_MISSING_NOTE, DEVICE_WRONG_NOTE] {
            let model = s7_with_device(DeviceUnlock::Off, Some(note));
            assert!(!shows(&model, size, "Unlock with Face ID"), "no button");
            let drawn = texts(view(&model), size);
            let top = |text: &str| {
                drawn
                    .iter()
                    .find(|(shown, _)| shown == text)
                    .map(|(_, bounds)| bounds.y)
                    .unwrap_or_else(|| panic!("no text {text:?}"))
            };
            assert!(top(note) < top("Password"), "the note is above the field");
        }
    }

    #[test]
    fn s7_draws_the_device_button_above_the_password_field_and_its_note_under_it() {
        let note = "The device could not release the password (the keychain is locked).";
        let model = s7_with_device(DeviceUnlock::On, Some(note));
        let drawn = texts(view(&model), (NARROWEST, 844.0));
        let top = |text: &str| {
            drawn
                .iter()
                .find(|(drawn, _)| drawn == text)
                .map(|(_, bounds)| bounds.y)
                .unwrap_or_else(|| panic!("no text {text:?}"))
        };
        let button = top("Unlock with Face ID");
        assert!(button < top(note), "the note is under the button");
        assert!(top(note) < top("Password"), "the field is under the note");
    }

    #[test]
    fn a_tap_on_the_device_button_asks_for_the_device_unlock() {
        let model = s7_with_device(DeviceUnlock::On, None);
        let size = (NARROWEST, 844.0);
        let (_, bounds) = texts(view(&model), size)
            .into_iter()
            .find(|(drawn, _)| drawn == "Unlock with Face ID")
            .expect("the button");
        let (x, y) = (
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        );
        let mut driver = Driver::new(view(&model), (360, 844));
        let (messages, _, _) = driver.send(&[mouse_move(x, y), mouse_press(), mouse_release()]);
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, Message::UnlockWithDevice)),
            "{messages:?}"
        );
    }

    #[test]
    fn the_device_button_and_its_note_fit_a_phone_of_360_by_640_and_390_by_844() {
        // The page scrolls, so nothing is held to the foot of the window, but
        // nothing reaches past the width at either size, with the button and
        // what the device unlock last found drawn.
        let note = "The password kept on this device no longer opens this store, so it has \
                    been deleted and the unlock with the device is off. Type the password to \
                    unlock.";
        for (width, height) in [(NARROWEST, 640.0), (390.0, 844.0)] {
            let mut model = s7_with_device(DeviceUnlock::On, Some(note));
            model.width = width;
            assert_eq!(
                fits_at(view(&model), width, height),
                Ok(()),
                "the unlock screen with the device button, at {width} by {height}"
            );
        }
    }

    /// The insets of a phone: 47 pixels at the top, where the status bar is,
    /// and 34 at the bottom, where the home indicator is, as the screenshot
    /// example and the phone's shell report them.
    const PHONE_INSETS: Padding = Padding {
        top: 47.0,
        right: 0.0,
        bottom: 34.0,
        left: 0.0,
    };

    /// The height of a window whose page, between a phone's insets, is `usable`
    /// pixels tall.
    fn window_of(usable: f32) -> f32 {
        usable + PHONE_INSETS.top + PHONE_INSETS.bottom
    }

    /// The phrase screen's state for the tests: the example words, the
    /// checkbox left empty, and the words shown after the warning if `shown`.
    fn phrase_state(shown: bool) -> PhraseState {
        PhraseState {
            phrase: PhraseForDisplay::example(),
            dir: String::new(),
            positions: CONFIRM_POSITIONS,
            written: false,
            words: Default::default(),
            error: None,
            shown,
            photographed: false,
        }
    }

    /// The phrase screen on a phone `width` wide, with the phone's insets.
    fn phone_phrase_model(width: f32, shown: bool) -> Model {
        let mut model = phone_model(Screen::Phrase(phrase_state(shown)));
        model.width = width;
        model.safe_area = SafeArea::new(PHONE_INSETS);
        model
    }

    /// The phrase screen on the desktop, `width` wide.
    fn desktop_phrase_model(width: f32) -> Model {
        let mut model = Model::new(Preferences::default(), None);
        model.width = width;
        model.screen = Screen::Phrase(phrase_state(false));
        model
    }

    /// The texts `model` draws in a window of `size`, in the order they are drawn.
    fn words_drawn(model: &Model, size: (f32, f32)) -> Vec<String> {
        texts(crate::screens::view(model), size)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    /// The boxes of the words' numbers in a drawing, one for each word: each
    /// number stands at the left of its cell.
    fn number_cells(drawn: &[(String, Rectangle)]) -> Vec<Rectangle> {
        drawn
            .iter()
            .filter(|(text, _)| {
                text.trim()
                    .parse::<usize>()
                    .is_ok_and(|n| (1..=24).contains(&n))
            })
            .map(|(_, at)| *at)
            .collect()
    }

    /// The boxes grouped into rows by their top edge, each row from the left.
    fn rows_of(boxes: &[Rectangle]) -> Vec<Vec<Rectangle>> {
        let mut tops: Vec<f32> = boxes.iter().map(|r| r.y).collect();
        tops.sort_by(f32::total_cmp);
        tops.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        tops.iter()
            .map(|top| {
                let mut row: Vec<Rectangle> = boxes
                    .iter()
                    .copied()
                    .filter(|r| (r.y - top).abs() < 0.5)
                    .collect();
                row.sort_by(|a, b| a.x.total_cmp(&b.x));
                row
            })
            .collect()
    }

    #[test]
    fn the_phrase_reads_down_three_columns_of_eight_rows_on_every_size() {
        // One grid on a phone and on the desktop.
        let grid = phrase_grid(24, PHRASE_COLUMNS);
        assert_eq!(grid.len(), 8, "eight rows");
        assert!(
            grid.iter().all(|cells| cells.len() == 3),
            "three cells in each row"
        );
        assert_eq!(
            grid[0],
            [Some(0), Some(8), Some(16)],
            "word 1 heads the first column, word 9 the second, word 17 the third"
        );
        assert_eq!(
            grid[7],
            [Some(7), Some(15), Some(23)],
            "word 8 ends the first column, word 16 the second, word 24 the third"
        );
    }

    #[test]
    fn the_drawn_phrase_is_eight_rows_of_three_numbered_cells() {
        for (name, model, size) in [
            (
                "a phone of 390 by 844",
                phone_phrase_model(390.0, false),
                (390.0, 844.0),
            ),
            (
                "the desktop at 1024 by 700",
                desktop_phrase_model(1024.0),
                (1024.0, 700.0),
            ),
        ] {
            let drawn = texts(crate::screens::view(&model), size);
            let cells = number_cells(&drawn);
            assert_eq!(cells.len(), 24, "{name}: one number per word");
            let rows = rows_of(&cells);
            assert_eq!(rows.len(), 8, "{name}: eight rows");
            assert!(
                rows.iter().all(|row| row.len() == 3),
                "{name}: three cells in each row"
            );
        }
    }

    #[test]
    fn a_phone_takes_one_frame_only_when_its_page_is_tall_enough() {
        assert_eq!(
            phrase_frame(PHRASE_ONE_FRAME, false, false),
            PhraseFrame::Whole
        );
        assert_eq!(
            phrase_frame(PHRASE_ONE_FRAME, true, false),
            PhraseFrame::Whole
        );
        let short = PHRASE_ONE_FRAME - 0.5;
        assert_eq!(phrase_frame(short, false, false), PhraseFrame::Warning);
        assert_eq!(phrase_frame(short, true, false), PhraseFrame::Phrase);
        // A screenshot hides the words, and the short phone keeps its frame.
        assert_eq!(phrase_frame(short, false, true), PhraseFrame::Phrase);
    }

    /// The phrase screen for `model`, its phrase 24 words of "abstract", in the
    /// root that `screens::view` gives every screen: the page padded by the
    /// phone's `insets`. "abstract" is the longest word of the library's list,
    /// eight letters, so these are the widest words a phrase can be.
    fn widest_phrase(model: &Model, insets: Padding) -> Element<'_, Message> {
        container(view_of(model, vec!["abstract"; 24]))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(insets)
            .into()
    }

    #[test]
    fn the_phrase_fits_the_window_at_every_size_the_plan_names() {
        // At 360 by 640, 360 by 800, 390 by 844 and the
        // desktop's 1024 by 700, every word is inside the part of the window a
        // person sees, the page does not scroll, and every word fits its cell.
        // Each size is held to it twice: with the example phrase, as the
        // application draws it, and with the widest words of the library's
        // list. The last column says whether the frame shows the words.
        let none = Padding::ZERO;
        let cases = [
            (
                "360 by 640, the warning",
                phone_phrase_model(360.0, false),
                (360.0, 640.0),
                PHONE_INSETS,
                false,
            ),
            (
                "360 by 640, the words after the warning",
                phone_phrase_model(360.0, true),
                (360.0, 640.0),
                PHONE_INSETS,
                true,
            ),
            (
                "360 by 800, the warning",
                phone_phrase_model(360.0, false),
                (360.0, 800.0),
                PHONE_INSETS,
                false,
            ),
            (
                "360 by 800, the words after the warning",
                phone_phrase_model(360.0, true),
                (360.0, 800.0),
                PHONE_INSETS,
                true,
            ),
            (
                "360 by the one-frame height, the warning and the words",
                phone_phrase_model(360.0, false),
                (360.0, window_of(PHRASE_ONE_FRAME)),
                PHONE_INSETS,
                true,
            ),
            (
                "390 by 844, the warning and the words",
                phone_phrase_model(390.0, false),
                (390.0, 844.0),
                PHONE_INSETS,
                true,
            ),
            (
                "the desktop at 1024 by 700",
                desktop_phrase_model(1024.0),
                (1024.0, 700.0),
                none,
                true,
            ),
        ];
        for (name, model, size, insets, shows_words) in cases {
            let problems = fits_the_viewport(crate::screens::view(&model), size, insets);
            assert!(
                problems.is_empty(),
                "{name}, the example phrase: {problems:#?}"
            );
            let widest = texts(widest_phrase(&model, insets), size)
                .into_iter()
                .filter(|(text, _)| text == "abstract")
                .count();
            assert_eq!(
                widest,
                if shows_words { 24 } else { 0 },
                "{name}: the words drawn"
            );
            let problems = fits_the_viewport(widest_phrase(&model, insets), size, insets);
            assert!(
                problems.is_empty(),
                "{name}, the widest words: {problems:#?}"
            );
        }
    }

    #[test]
    fn the_welcome_screen_fits_each_phone_the_plan_names_between_its_insets() {
        // The welcome screen does not scroll, and each of its words is
        // within the part of the window a person sees, at 360 by 640, 360 by
        // 800 and 390 by 844, between the phone's insets as the shell reports
        // them. iced fits a child into the room its parent gives it, so a
        // screen too tall shows as a squeezed child, not as a word past the
        // inset: the button keeps its full size, and its foot is the page's
        // bottom margin above the bottom inset, which holds only if everything
        // above it fits.
        for (width, height) in [(360.0, 640.0), (360.0, 800.0), (390.0, 844.0)] {
            let mut model = phone_model(Screen::Welcome);
            model.width = width;
            model.safe_area = SafeArea::new(PHONE_INSETS);
            let problems =
                fits_the_viewport(crate::screens::view(&model), (width, height), PHONE_INSETS);
            assert!(problems.is_empty(), "{width} by {height}: {problems:#?}");
            let button = drawn_bounds(crate::screens::view(&model), width, height)
                .into_iter()
                .find(|r| (r.width - (width - 32.0)).abs() < 0.5 && (r.height - 56.0).abs() < 0.5)
                .unwrap_or_else(|| panic!("{width} by {height}: the button, at its full size"));
            assert!(
                (button.y + button.height - (height - PHONE_INSETS.bottom - 24.0)).abs() < 0.5,
                "{width} by {height}: the button does not stand at the foot: {button:?}"
            );
        }
    }

    #[test]
    fn a_short_phone_shows_the_warning_alone_and_then_the_words() {
        // On a phone shorter than one frame, the library's
        // warning comes first, alone, with "Show the phrase"; the words, the
        // checkbox and Continue follow it, and there is no wordmark.
        let warning = words_drawn(&phone_phrase_model(360.0, false), (360.0, 640.0));
        assert!(warning.iter().any(|t| t == PHRASE_TITLE), "{warning:?}");
        assert!(
            warning.iter().any(|t| t == "Show the phrase"),
            "{warning:?}"
        );
        assert!(
            !warning
                .iter()
                .any(|t| t == "Continue" || t == "abandon" || t == "art"),
            "the words wait for the button: {warning:?}"
        );
        assert!(!warning.iter().any(|t| t == "TAWARA"), "{warning:?}");

        let words = words_drawn(&phone_phrase_model(360.0, true), (360.0, 640.0));
        assert!(words.iter().any(|t| t == "Continue"), "{words:?}");
        assert!(
            words.iter().any(|t| t == "Start over with a new phrase"),
            "{words:?}"
        );
        assert!(words.iter().any(|t| t == "abandon"), "{words:?}");
        assert!(
            !words
                .iter()
                .any(|t| t == PHRASE_TITLE || t == "Show the phrase"),
            "the warning and its button are not drawn with the words: {words:?}"
        );
    }

    #[test]
    fn show_the_phrase_asks_for_the_words() {
        let model = phone_phrase_model(360.0, false);
        let mut driver = Driver::new(crate::screens::view(&model), (360, 640));
        let at = driver
            .text_bounds("Show the phrase")
            .expect("the button is drawn");
        let (messages, _, _) = driver.send(&[
            mouse_move(at.center_x(), at.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(
            matches!(messages.as_slice(), [Message::ShowPhrase]),
            "{messages:?}"
        );
    }

    #[test]
    fn the_warning_and_the_phrase_share_one_frame_from_the_threshold_up() {
        // The one frame is drawn from PHRASE_ONE_FRAME pixels up; a page one
        // pixel shorter draws the warning alone.
        let one = words_drawn(
            &phone_phrase_model(360.0, false),
            (360.0, window_of(PHRASE_ONE_FRAME)),
        );
        assert!(one.iter().any(|t| t == PHRASE_TITLE), "{one:?}");
        assert!(one.iter().any(|t| t == "Continue"), "{one:?}");
        assert!(!one.iter().any(|t| t == "Show the phrase"), "{one:?}");

        let two = words_drawn(
            &phone_phrase_model(360.0, false),
            (360.0, window_of(PHRASE_ONE_FRAME - 1.0)),
        );
        assert!(two.iter().any(|t| t == "Show the phrase"), "{two:?}");
        assert!(!two.iter().any(|t| t == "Continue"), "{two:?}");
    }

    #[test]
    fn the_desktop_draws_the_warning_before_the_words_on_one_frame() {
        let model = desktop_phrase_model(1024.0);
        let drawn = texts(crate::screens::view(&model), (1024.0, 700.0));
        let at = |text: &str| drawn.iter().find(|(t, _)| t == text).map(|(_, r)| *r);
        let title = at(PHRASE_TITLE).expect("the warning is drawn");
        let first = at("abandon").expect("the first word is drawn");
        let last = at("art").expect("the last word is drawn");
        let next = at("Continue").expect("Continue is drawn");
        assert!(
            title.y < first.y && first.y < last.y && last.y < next.y,
            "the warning, then the words, then Continue: {title:?} {first:?} {last:?} {next:?}"
        );
        assert!(
            !drawn.iter().any(|(t, _)| t == "Show the phrase"),
            "the desktop has one frame"
        );
    }

    /// The phrase screen on a phone `width` wide after a screenshot taken while
    /// its words were drawn: the words are hidden until "Show the phrase" is
    /// pressed.
    fn shot_phrase_model(width: f32) -> Model {
        let mut model = phone_phrase_model(width, true);
        if let Screen::Phrase(p) = &mut model.screen {
            p.photographed = true;
            p.shown = false;
        }
        model
    }

    /// The box `text` is drawn in, in a window of `size` with `model` on it.
    fn drawn_at(model: &Model, size: (f32, f32), text: &str) -> Rectangle {
        texts(crate::screens::view(model), size)
            .into_iter()
            .find(|(drawn, _)| drawn == text)
            .map(|(_, at)| at)
            .unwrap_or_else(|| panic!("{text} is not drawn"))
    }

    /// The window sizes the warned page is held to: the two short phones and
    /// the one-frame phone.
    const WARNED_SIZES: [(f32, (f32, f32)); 3] = [
        (360.0, (360.0, 640.0)),
        (360.0, (360.0, 800.0)),
        (390.0, (390.0, 844.0)),
    ];

    #[test]
    fn a_screenshot_takes_the_words_place_until_show_is_pressed() {
        // After a screenshot taken while the
        // words are drawn, the warning takes their place, with "Show the phrase",
        // and the checkbox and Continue stay. The words come back when the button
        // is pressed, and the warning goes with them.
        for (width, size) in WARNED_SIZES {
            let before = words_drawn(&phone_phrase_model(width, true), size);
            assert!(
                !before.iter().any(|t| t == PHRASE_SHOT_TITLE),
                "{size:?}: no warning before a screenshot: {before:?}"
            );
            let mut model = shot_phrase_model(width);
            let hidden = words_drawn(&model, size);
            for text in [
                PHRASE_SHOT_TITLE,
                PHRASE_SHOT_BODY,
                "Show the phrase",
                "Continue",
                "Start over with a new phrase",
            ] {
                assert!(
                    hidden.iter().any(|t| t == text),
                    "{size:?}: {text} is drawn: {hidden:?}"
                );
            }
            assert!(
                !hidden.iter().any(|t| t == "abandon" || t == "art"),
                "{size:?}: no word is drawn: {hidden:?}"
            );
            if let Screen::Phrase(p) = &mut model.screen {
                p.shown = true;
            }
            let back = words_drawn(&model, size);
            assert!(
                back.iter().any(|t| t == "abandon"),
                "{size:?}: the words are back: {back:?}"
            );
            assert!(
                !back
                    .iter()
                    .any(|t| t == PHRASE_SHOT_TITLE || t == "Show the phrase"),
                "{size:?}: the warning goes with them: {back:?}"
            );
        }
    }

    #[test]
    fn the_warning_takes_the_grids_place_and_the_page_does_not_move() {
        // The checkbox, Continue and the link
        // stand where they stand with the words, so the page does not move.
        for (width, size) in WARNED_SIZES {
            let shown = phone_phrase_model(width, true);
            let hidden = shot_phrase_model(width);
            for text in [
                "I have written down every word, in order.",
                "Continue",
                "Start over with a new phrase",
            ] {
                let with_words = drawn_at(&shown, size, text);
                let with_warning = drawn_at(&hidden, size, text);
                assert!(
                    (with_words.y - with_warning.y).abs() < 0.5,
                    "{size:?}: {text} is at {} with the words, {} with the warning",
                    with_words.y,
                    with_warning.y
                );
            }
        }
    }

    #[test]
    fn the_desktop_keeps_its_warning_above_the_words_after_a_screenshot() {
        // No phone shell confines the desktop, and no shell reports a
        // screenshot on it. Where one is noted, the desktop's warning comes
        // above the words on its one frame, and there is no "Show the phrase"
        // button.
        let mut model = desktop_phrase_model(1024.0);
        if let Screen::Phrase(p) = &mut model.screen {
            p.photographed = true;
        }
        let drawn = texts(crate::screens::view(&model), (1024.0, 700.0));
        let at = |text: &str| drawn.iter().find(|(t, _)| t == text).map(|(_, r)| *r);
        let warning = at(PHRASE_SHOT_TITLE).expect("the warning is drawn");
        let first = at("abandon").expect("the first word is drawn");
        assert!(
            warning.y < first.y,
            "the warning, then the words: {warning:?} {first:?}"
        );
        assert!(
            !drawn.iter().any(|(t, _)| t == "Show the phrase"),
            "the desktop has one frame"
        );
    }

    #[test]
    fn a_screenshot_takes_the_words_place_on_a_phone_held_sideways() {
        // A phone held sideways is 844 by 390
        // and draws the desktop's frame. The iOS shell reports its screenshots in
        // any orientation, so the warning takes the words' place there as it does
        // on a tall phone, until "Show the phrase" is pressed. The checkbox and
        // Continue do not move, and the words come back with no warning.
        fn s4(model: &Model) -> &PhraseState {
            match &model.screen {
                Screen::Phrase(p) => p,
                _ => panic!("the recovery phrase screen is shown"),
            }
        }
        let size = (844.0, 390.0);
        let mut model = phone_phrase_model(844.0, false);
        model.height = 390.0;
        let drawn = words_drawn(&model, size);
        assert!(drawn.iter().any(|t| t == "abandon"), "the words: {drawn:?}");
        assert!(
            phrase_words_drawn(&model, s4(&model)),
            "the words are drawn before a screenshot"
        );
        if let Screen::Phrase(p) = &mut model.screen {
            p.photographed = true;
        }
        assert!(
            !phrase_words_drawn(&model, s4(&model)),
            "a screenshot hides the words, and they are not noted again"
        );
        let hidden = words_drawn(&model, size);
        for text in [
            PHRASE_SHOT_TITLE,
            PHRASE_SHOT_BODY,
            "Show the phrase",
            "Continue",
            "Start over with a new phrase",
        ] {
            assert!(
                hidden.iter().any(|t| t == text),
                "{text} is drawn: {hidden:?}"
            );
        }
        assert!(
            !hidden.iter().any(|t| t == "abandon"),
            "no word is drawn: {hidden:?}"
        );
        let mut shown_model = phone_phrase_model(844.0, true);
        shown_model.height = 390.0;
        for text in [
            "I have written down every word, in order.",
            "Continue",
            "Start over with a new phrase",
        ] {
            let at_words = drawn_at(&shown_model, size, text);
            let at_warning = drawn_at(&model, size, text);
            assert!(
                (at_words.y - at_warning.y).abs() < 0.5,
                "{text} is at {} with the words, {} with the warning",
                at_words.y,
                at_warning.y
            );
        }
        if let Screen::Phrase(p) = &mut model.screen {
            p.shown = true;
        }
        let back = words_drawn(&model, size);
        assert!(
            back.iter().any(|t| t == "abandon"),
            "the words are back: {back:?}"
        );
        assert!(
            !back
                .iter()
                .any(|t| t == PHRASE_SHOT_TITLE || t == "Show the phrase"),
            "the warning goes with them: {back:?}"
        );
    }

    #[test]
    fn the_words_are_blanked_while_the_screen_is_captured_and_come_back_after() {
        // While the screen is recorded or
        // mirrored, a placeholder stands where the words are, and the checkbox
        // and Continue stay. When it stops, the words come back.
        for (name, mut model, size) in [
            (
                "a phone of 390 by 844",
                phone_phrase_model(390.0, false),
                (390.0, 844.0),
            ),
            (
                "a phone of 360 by 640 with the words shown",
                phone_phrase_model(360.0, true),
                (360.0, 640.0),
            ),
            (
                "the desktop at 1024 by 700",
                desktop_phrase_model(1024.0),
                (1024.0, 700.0),
            ),
        ] {
            model.captured = true;
            let drawn = words_drawn(&model, size);
            assert!(
                drawn.iter().any(|t| t == PHRASE_HIDDEN),
                "{name}: {drawn:?}"
            );
            assert!(
                !drawn.iter().any(|t| t == "abandon" || t == "art"),
                "{name}: no word is drawn: {drawn:?}"
            );
            assert!(drawn.iter().any(|t| t == "Continue"), "{name}: {drawn:?}");
            assert!(
                drawn
                    .iter()
                    .any(|t| t == "I have written down every word, in order."),
                "{name}: {drawn:?}"
            );
            model.captured = false;
            let back = words_drawn(&model, size);
            assert!(back.iter().any(|t| t == "abandon"), "{name}: {back:?}");
            assert!(!back.iter().any(|t| t == PHRASE_HIDDEN), "{name}: {back:?}");
        }
    }

    #[test]
    fn the_placeholder_is_as_tall_as_the_grid_it_replaces() {
        // The page does not move when the
        // words are hidden, so the placeholder, and the warning that takes the
        // words' place after a screenshot, are the grid's height.
        let phrase = PhraseForDisplay::example();
        let words: Vec<&str> = phrase.words().collect();
        let grid = drawn_bounds(phone_block(words_grid(&words), 0.0, 0.0), 360.0, 800.0)[0];
        let hidden = drawn_bounds(phone_block(phrase_hidden(), 0.0, 0.0), 360.0, 800.0)[0];
        let shot = drawn_bounds(phone_block(phrase_shot(), 0.0, 0.0), 360.0, 800.0)[0];
        assert!(
            (grid.height - PHRASE_GRID_HEIGHT).abs() < 0.5,
            "the grid is {} tall",
            grid.height
        );
        assert!(
            (hidden.height - grid.height).abs() < 0.5,
            "the placeholder is {} tall, the grid {}",
            hidden.height,
            grid.height
        );
        assert!(
            (shot.height - grid.height).abs() < 0.5,
            "the warning is {} tall, the grid {}",
            shot.height,
            grid.height
        );
    }

    #[test]
    fn the_words_hidden_by_a_capture_fit_the_window_as_the_words_do() {
        // The placeholder keeps the frame's
        // size, so every word and button still fits where it fit before.
        let none = Padding::ZERO;
        for (name, mut model, size, insets) in [
            (
                "360 by 640, the words shown",
                phone_phrase_model(360.0, true),
                (360.0, 640.0),
                PHONE_INSETS,
            ),
            (
                "360 by 800",
                phone_phrase_model(360.0, true),
                (360.0, 800.0),
                PHONE_INSETS,
            ),
            (
                "390 by 844",
                phone_phrase_model(390.0, false),
                (390.0, 844.0),
                PHONE_INSETS,
            ),
            (
                "the desktop at 1024 by 700",
                desktop_phrase_model(1024.0),
                (1024.0, 700.0),
                none,
            ),
        ] {
            model.captured = true;
            let problems = fits_the_viewport(crate::screens::view(&model), size, insets);
            assert!(problems.is_empty(), "{name}: {problems:#?}");
        }
    }

    #[test]
    fn the_warned_page_fits_one_frame_at_360_by_640_360_by_800_and_390_by_844() {
        // With the warning in the words' place, the page after a screenshot is
        // one frame, with no scroll and no word or button past the part of the
        // window a person sees, at each size tested.
        for (width, size) in WARNED_SIZES {
            let model = shot_phrase_model(width);
            let problems = fits_the_viewport(crate::screens::view(&model), size, PHONE_INSETS);
            assert!(problems.is_empty(), "{size:?}: {problems:#?}");
        }
    }

    #[test]
    fn the_words_are_drawn_where_the_frame_draws_them() {
        // A screenshot counts only when
        // the words were drawn, so the model asks the same question the layout
        // answers, on the same heights.
        fn s4(model: &Model) -> &PhraseState {
            match &model.screen {
                Screen::Phrase(p) => p,
                other => panic!("expected the recovery phrase screen, got {other:?}"),
            }
        }
        let desktop = desktop_phrase_model(1024.0);
        assert!(phrase_words_drawn(&desktop, s4(&desktop)));

        let mut whole = phone_phrase_model(360.0, false);
        whole.height = window_of(PHRASE_ONE_FRAME);
        assert!(phrase_words_drawn(&whole, s4(&whole)));
        assert!(
            words_drawn(&whole, (360.0, window_of(PHRASE_ONE_FRAME)))
                .iter()
                .any(|t| t == "abandon")
        );

        let mut short = phone_phrase_model(360.0, false);
        short.height = window_of(PHRASE_ONE_FRAME - 1.0);
        assert!(!phrase_words_drawn(&short, s4(&short)));
        assert!(
            !words_drawn(&short, (360.0, window_of(PHRASE_ONE_FRAME - 1.0)))
                .iter()
                .any(|t| t == "abandon")
        );

        if let Screen::Phrase(p) = &mut short.screen {
            p.shown = true;
        }
        assert!(phrase_words_drawn(&short, s4(&short)));

        // A screenshot hides the words again on either frame, until "Show the
        // phrase" is pressed.
        let mut tall_shot = phone_phrase_model(360.0, false);
        tall_shot.height = window_of(PHRASE_ONE_FRAME);
        if let Screen::Phrase(p) = &mut tall_shot.screen {
            p.photographed = true;
        }
        assert!(!phrase_words_drawn(&tall_shot, s4(&tall_shot)));
        let mut short_shot = phone_phrase_model(360.0, true);
        short_shot.height = window_of(PHRASE_ONE_FRAME - 1.0);
        if let Screen::Phrase(p) = &mut short_shot.screen {
            p.photographed = true;
            p.shown = false;
        }
        assert!(!phrase_words_drawn(&short_shot, s4(&short_shot)));
    }

    #[test]
    fn the_desktop_draws_its_words_with_no_warning_when_no_shell_reports_a_capture() {
        // The desktop keeps no capture protection, and with no shell
        // reporting, its phrase is drawn unchanged.
        let model = desktop_phrase_model(1024.0);
        let drawn = words_drawn(&model, (1024.0, 700.0));
        assert!(drawn.iter().any(|t| t == "abandon"), "{drawn:?}");
        assert!(
            !drawn
                .iter()
                .any(|t| t == PHRASE_SHOT_TITLE || t == PHRASE_HIDDEN),
            "{drawn:?}"
        );
    }

    #[test]
    fn the_widest_word_fits_its_cell_on_a_phone_of_360() {
        // "abstract" is the longest word of the library's list, eight letters,
        // so the 24 of them in the grid are the widest a phrase can be.
        let words = ["abstract"; 24];
        let grid = phone_block(words_grid(&words), 0.0, 0.0);
        let problems = fits_the_viewport(grid, (360.0, 800.0), Padding::ZERO);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn a_tap_on_the_node_footers_link_opens_the_node_screen() {
        // On a phone the footer's link has the room of its own row, so a tap
        // on it reaches it, whether the node answers or not, on each screen
        // that has the footer.
        let node = |m: &Message| matches!(m, Message::Go(Go::Node(_)));
        let screens = || {
            [
                (
                    "start",
                    Screen::Start {
                        choice: StartChoice::Create,
                    },
                ),
                ("new wallet", Screen::NewWallet(PasswordForm::default())),
                ("unlock", Screen::Unlock(UnlockForm::default())),
            ]
        };
        for not_answering in [false, true] {
            for (name, screen) in screens() {
                let mut model = phone_model(screen);
                if not_answering {
                    model.node.error = Some("the API did not answer".to_owned());
                }
                // A window tall enough for the whole page, so that the footer
                // is on it without a drag.
                let mut driver = Driver::new(view(&model), (360, 1400));
                // The link ends at the right margin, 16 pixels from the edge.
                let span = tapped_rows(&mut driver, 320.0, 0..1400, node);
                let (top, bottom) = span.unwrap_or_else(|| {
                    panic!("{name}, not answering {not_answering}: no tap reached the link")
                });
                assert!(
                    bottom - top >= 10.0,
                    "{name}, not answering {not_answering}: the link is {top}..{bottom}"
                );
            }
        }
    }

    #[test]
    fn a_drag_from_a_radio_card_scrolls_the_page_and_a_tap_chooses() {
        let restore = |m: &Message| matches!(m, Message::Choose(StartChoice::Restore));
        let model = phone_model(Screen::Start {
            choice: StartChoice::Create,
        });
        // A window shorter than the page, so that a drag has something to scroll.
        let mut driver = Driver::new(view(&model), (360, 600));
        let x = 180.0;
        let (top, bottom) =
            tapped_rows(&mut driver, x, 0..600, restore).expect("the restore card is on the page");
        let middle = (top + bottom) / 2.0;

        // The finger goes down on the card and comes up 40 pixels higher.
        let (messages, _, _) = driver.send(&[
            finger_press(2, x, middle),
            finger_move(2, x, middle - 20.0),
            finger_move(2, x, middle - 40.0),
            finger_lift(2, x, middle - 40.0),
        ]);
        assert!(
            !messages.iter().any(|m| matches!(m, Message::Choose(_))),
            "a drag chose a card: {messages:?}"
        );

        // The page scrolled under the finger, so the card is 40 pixels higher.
        let (moved_top, moved_bottom) = tapped_rows(&mut driver, x, 0..600, restore)
            .expect("the restore card, on the scrolled page");
        assert!(
            (top - 40.0 - moved_top).abs() <= 4.0,
            "the page moved the card by {}, not by 40",
            top - moved_top
        );

        // On the scrolled page, a tap chooses the card.
        let middle = (moved_top + moved_bottom) / 2.0;
        let (messages, _, _) =
            driver.send(&[finger_press(3, x, middle), finger_lift(3, x, middle)]);
        assert!(
            messages.iter().any(restore),
            "a tap chose nothing: {messages:?}"
        );
    }

    #[test]
    fn s6_has_a_paste_button_by_the_phrase_on_a_phone_alone() {
        // The button asks for the phrase's paste; the desktop pastes from the
        // keyboard.
        let model = phone_model(Screen::Restore(RestoreForm::default()));
        let pastes: Vec<Rectangle> = texts(view(&model), (NARROWEST, 844.0))
            .into_iter()
            .filter(|(text, _)| text == "Paste")
            .map(|(_, at)| at)
            .collect();
        assert_eq!(pastes.len(), 1, "{pastes:?}");
        let at = pastes[0];
        let mut driver = Driver::new(view(&model), (360, 844));
        let (messages, _, _) = driver.send(&[
            mouse_move(at.center_x(), at.center_y()),
            mouse_press(),
            mouse_release(),
        ]);
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Paste(PasteInto::Phrase { drawn })] if *drawn == model.drawn()
            ),
            "{messages:?}"
        );

        // A phone held sideways pastes into the phrase too, as it does
        // portrait.
        let mut sideways = phone_model(Screen::Restore(RestoreForm::default()));
        sideways.width = 844.0;
        assert!(
            texts(view(&sideways), (844.0, 844.0))
                .iter()
                .any(|(text, _)| text == "Paste")
        );

        let mut desktop = Model::new(Preferences::default(), None);
        desktop.screen = Screen::Restore(RestoreForm::default());
        assert!(
            !texts(view(&desktop), (1440.0, 900.0))
                .iter()
                .any(|(text, _)| text == "Paste")
        );
    }

    #[test]
    fn a_keyboard_paste_into_the_phrase_field_keeps_the_words_apart() {
        // On the restore screen: a phrase copied one
        // word to a line arrives as the words, separated, on the phone and on
        // the desktop alike, and as the one field's text.
        let phone = phone_model(Screen::Restore(RestoreForm::default()));
        let mut desktop = Model::new(Preferences::default(), None);
        desktop.screen = Screen::Restore(RestoreForm::default());
        for (model, size) in [(&phone, (360, 844)), (&desktop, (1440, 900))] {
            let mut driver = Driver::new(view(model), size);
            driver.operate(Box::new(focusable::focus(crate::screens::phrase_id())));
            let messages = driver.send_with(&paste_keys(), &mut Holding("one\ntwo\n\nthree"));
            assert!(
                matches!(
                    messages.as_slice(),
                    [Message::PhraseText(typed)] if typed.text == "one two three"
                ),
                "{messages:?}"
            );
        }
    }

    #[test]
    fn a_node_not_asked_yet_is_not_drawn_as_one_that_answered() {
        // At start-up the default node is set and asked nothing until a wallet
        // opens: the start and unlock screens must not draw it as a node that
        // answered. Each label is drawn whole, and only its own.
        let labels = [
            "Mesh API",
            "API Not Yet Connected",
            "Mesh API, not answering",
        ];
        let answered = Some((871_173, std::time::Duration::from_millis(42)));
        let failed = Some("the API did not answer".to_owned());
        for (screen, tip, error, label) in [
            ("S1", None, None, labels[1]),
            ("S7", None, None, labels[1]),
            ("S1", answered, None, labels[0]),
            ("S1", None, failed.clone(), labels[2]),
            ("S1", answered, failed, labels[2]),
        ] {
            let mut model = named(screen);
            model.node.tip = tip;
            model.node.error = error;
            let drawn: Vec<String> = texts(view(&model), (NARROWEST, 1400.0))
                .into_iter()
                .map(|(text, _)| text)
                .collect();
            for other in labels {
                assert_eq!(
                    drawn.iter().any(|t| t == other),
                    other == label,
                    "{screen}, tip {tip:?}: `{other}` in {drawn:?}"
                );
            }
        }
    }
}
