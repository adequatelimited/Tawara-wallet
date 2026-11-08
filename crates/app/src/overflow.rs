// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The check that nothing a phone screen draws passes the edge of its window.
//! The screenshot example's `--fit` and the screens' tests both use it.
//!
//! iced clamps each child to the room its parent gives it, so an oversized
//! node's box does not pass the edge, but the words of a text that cannot
//! break are still drawn past their box. The check therefore reads the width
//! those words need from the paragraph the tree holds for the text. Only what
//! passes the window edge is reported: a label squeezed inside a button whose
//! words stay inside the window is not.
//!
//! [`viewport_problems`] is stricter, for screens that must show all their
//! words at once: every word stays between the system insets, the page does
//! not scroll, and every word fits the box it is drawn in.

use iced::advanced::Layout;
use iced::advanced::layout;
use iced::advanced::widget::operation::Scrollable;
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::{Element, Padding, Point, Rectangle, Size, Vector};

/// The paragraph the screens' text is laid out in, for the renderer the
/// application draws with.
type Paragraph = <iced::Renderer as iced::advanced::text::Renderer>::Paragraph;

/// Something a screen draws past the right edge of its window.
#[derive(Clone, Debug, PartialEq)]
pub struct Past {
    /// Where it lies in the window. For a text, the box starts where the text's
    /// box starts and is as wide as the words it needs.
    pub bounds: Rectangle,
    /// The content of a text whose words run past the edge.
    pub text: Option<String>,
}

/// What `root` draws past the right edge of a window `width` logical pixels
/// wide and `height` tall, as `renderer` lays it out at that size: every node
/// whose right edge passes `width` plus half a pixel, and every text whose
/// words do. Positions are absolute (each node's offset added to its
/// parent's).
#[must_use]
pub fn past_the_edge<'a, M: 'a>(
    root: Element<'a, M>,
    renderer: &iced::Renderer,
    width: f32,
    height: f32,
) -> Vec<Past> {
    let measured = measure(root, renderer, width, height);
    let mut past = Vec::new();
    nodes_past(&measured.node, Point::ORIGIN, width, &mut past);
    past.extend(
        squeezed(&measured.texts, &measured.needs, width)
            .into_iter()
            .map(|(bounds, need, text)| Past {
                bounds: Rectangle::new(bounds.position(), Size::new(need, bounds.height)),
                text: Some(text),
            }),
    );
    past
}

/// What `root` draws outside the visible part of a window of `size`, one
/// problem per line (empty when there are none):
///
/// - a node drawn past the right edge;
/// - a word drawn past the right edge, above the top inset or below the window
///   less the bottom inset, or needing more width than the box it is drawn in
///   (a word wider than its cell);
/// - a page whose content is taller than its bounds, and so scrolls.
///
/// `insets` are what the system covers at the top and bottom: the status bar,
/// the home indicator, the keyboard. The desktop passes none.
#[must_use]
pub fn viewport_problems<'a, M: 'a>(
    root: Element<'a, M>,
    renderer: &iced::Renderer,
    size: Size,
    insets: Padding,
) -> Vec<String> {
    let measured = measure(root, renderer, size.width, size.height);
    let mut problems = Vec::new();
    let mut past = Vec::new();
    nodes_past(&measured.node, Point::ORIGIN, size.width, &mut past);
    for node in &past {
        problems.push(format!(
            "a node is drawn past the right edge: {}",
            describe(node.bounds)
        ));
    }
    let top = insets.top;
    let bottom = size.height - insets.bottom;
    for (text, bounds, need) in paired(&measured.texts, &measured.needs) {
        if bounds.x + need > size.width + 0.5 {
            problems.push(format!(
                "{text:?} is drawn past the right edge: {}",
                describe(bounds)
            ));
        }
        if bounds.y < top - 0.5 {
            problems.push(format!(
                "{text:?} is drawn above the top inset at {top:.1}: {}",
                describe(bounds)
            ));
        }
        if bounds.y + bounds.height > bottom + 0.5 {
            problems.push(format!(
                "{text:?} is drawn below the bottom inset at {bottom:.1}: {}",
                describe(bounds)
            ));
        }
        if need > bounds.width + 0.5 {
            problems.push(format!(
                "{text:?} needs {need:.1} pixels and is drawn in {:.1}: {}",
                bounds.width,
                describe(bounds)
            ));
        }
    }
    for (bounds, content) in &measured.scrolls {
        if content.height > bounds.height + 0.5 {
            problems.push(format!(
                "a page {:.1} pixels tall is drawn in {:.1}, so it scrolls",
                content.height, bounds.height
            ));
        }
    }
    problems
}

/// A rectangle as the problems name it: its extent across and down.
fn describe(r: Rectangle) -> String {
    format!(
        "x {:.1}..{:.1}, y {:.1}..{:.1}",
        r.x,
        r.x + r.width,
        r.y,
        r.y + r.height
    )
}

/// What a layout of a root draws, measured in one pass: the root node, every
/// text with its box, the width its words need, and every scrollable with its
/// bounds and its content.
struct Measured {
    node: layout::Node,
    texts: Vec<(String, Rectangle)>,
    needs: Vec<(String, f32)>,
    scrolls: Vec<(Rectangle, Rectangle)>,
}

/// Lays `root` out in a window `width` by `height` as `renderer` does, and
/// measures what it draws.
fn measure<'a, M: 'a>(
    mut root: Element<'a, M>,
    renderer: &iced::Renderer,
    width: f32,
    height: f32,
) -> Measured {
    let mut tree = Tree::new(root.as_widget());
    let limits = layout::Limits::new(Size::ZERO, Size::new(width, height));
    let node = root.as_widget_mut().layout(&mut tree, renderer, &limits);
    let mut seen = Seen::default();
    root.as_widget_mut()
        .operate(&mut tree, Layout::new(&node), renderer, &mut seen);
    let mut needs = Vec::new();
    text_needs(&tree, &mut needs);
    Measured {
        node,
        texts: seen.texts,
        needs,
        scrolls: seen.scrolls,
    }
}

/// Collects, under `node` at `origin`, every node whose right edge passes
/// `width` plus half a pixel.
fn nodes_past(node: &layout::Node, origin: Point, width: f32, past: &mut Vec<Past>) {
    for child in node.children() {
        let bounds = child.bounds();
        let at = Point::new(origin.x + bounds.x, origin.y + bounds.y);
        let absolute = Rectangle::new(at, bounds.size());
        if absolute.x + absolute.width > width + 0.5 {
            past.push(Past {
                bounds: absolute,
                text: None,
            });
        }
        nodes_past(child, at, width, past);
    }
}

/// Every text, and every scrollable, an operation meets, with their boxes.
#[derive(Default)]
struct Seen {
    texts: Vec<(String, Rectangle)>,
    scrolls: Vec<(Rectangle, Rectangle)>,
}

impl Operation for Seen {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
        self.texts.push((text.to_owned(), bounds));
    }

    fn scrollable(
        &mut self,
        _: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        _: Vector,
        _: &mut dyn Scrollable,
    ) {
        self.scrolls.push((bounds, content));
    }
}

/// Every text `tree` holds a paragraph for, in the order the tree holds them,
/// with the width its widest line takes. For words that cannot break, that is
/// more than the box they were given when they are squeezed into it.
fn text_needs(tree: &Tree, out: &mut Vec<(String, f32)>) {
    if tree.tag == tree::Tag::of::<iced::widget::text::State<Paragraph>>() {
        let state = tree
            .state
            .downcast_ref::<iced::widget::text::State<Paragraph>>();
        out.push((state.content().to_owned(), state.min_width()));
    }
    for child in &tree.children {
        text_needs(child, out);
    }
}

/// Each text with its box and the width its words need, paired in order: the
/// operation and the tree visit texts in the same order, so a box is paired
/// with the next paragraph of the same content. A paragraph with no box (a
/// checkbox label, which the operation does not report) is skipped unless a
/// later box has the same content, in which case that box is measured by the
/// label's paragraph. No screen labels a checkbox with another of its texts.
fn paired(boxes: &[(String, Rectangle)], needs: &[(String, f32)]) -> Vec<(String, Rectangle, f32)> {
    let mut out = Vec::new();
    let mut next = 0;
    for (text, bounds) in boxes {
        let Some(found) = needs[next..]
            .iter()
            .position(|(content, _)| content == text)
        else {
            continue;
        };
        let need = needs[next + found].1;
        next += found + 1;
        out.push((text.clone(), *bounds, need));
    }
    out
}

/// The texts whose words run past the right edge of a window `width` wide, each
/// with its box, the width its words need and its content. A text squeezed into
/// its box whose words stay inside the window is not reported.
fn squeezed(
    boxes: &[(String, Rectangle)],
    needs: &[(String, f32)],
    width: f32,
) -> Vec<(Rectangle, f32, String)> {
    paired(boxes, needs)
        .into_iter()
        .filter(|(_, bounds, need)| bounds.x + need > width + 0.5)
        .map(|(text, bounds, need)| (bounds, need, text))
        .collect()
}

#[cfg(test)]
mod tests {
    use iced::widget::{column, container, scrollable, space, text};
    use iced::{Length, Padding, Size};

    use super::*;
    use crate::harness::renderer;

    /// The insets of a phone with a status bar of 47 pixels at the top and a
    /// home indicator of 34 at the bottom, as the screenshot example sets them.
    const PHONE_INSETS: Padding = Padding {
        top: 47.0,
        right: 0.0,
        bottom: 34.0,
        left: 0.0,
    };

    #[test]
    fn a_word_below_the_bottom_inset_is_a_problem_and_one_above_it_is_not() {
        // A window 640 tall with 34 pixels covered at the bottom ends its
        // words at 606. A word at 600 runs past that; one at 580 does not.
        let below: Element<'_, ()> =
            column![space().height(Length::Fixed(600.0)), text("below")].into();
        let problems = viewport_problems(below, &renderer(), Size::new(360.0, 640.0), PHONE_INSETS);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("\"below\"") && p.contains("bottom inset")),
            "{problems:?}"
        );

        let above: Element<'_, ()> =
            column![space().height(Length::Fixed(580.0)), text("above")].into();
        let problems = viewport_problems(above, &renderer(), Size::new(360.0, 640.0), PHONE_INSETS);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn the_bottom_inset_counts_and_the_window_without_it_would_not_show_the_word() {
        // The same word at 600 is inside a window that the system covers
        // nothing of: the inset is what makes it a problem.
        let word: Element<'_, ()> =
            column![space().height(Length::Fixed(600.0)), text("below")].into();
        let problems = viewport_problems(word, &renderer(), Size::new(360.0, 640.0), Padding::ZERO);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_word_above_the_top_inset_is_a_problem() {
        let high: Element<'_, ()> = text("high").into();
        let problems = viewport_problems(high, &renderer(), Size::new(360.0, 640.0), PHONE_INSETS);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("\"high\"") && p.contains("top inset")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_page_taller_than_its_window_is_a_problem() {
        // The page's content is 900 tall, in a window 640 tall: it scrolls.
        let tall: Element<'_, ()> = scrollable(space().height(Length::Fixed(900.0))).into();
        let problems = viewport_problems(tall, &renderer(), Size::new(360.0, 640.0), Padding::ZERO);
        assert!(
            problems.iter().any(|p| p.contains("scrolls")),
            "{problems:?}"
        );

        let short: Element<'_, ()> = scrollable(space().height(Length::Fixed(300.0))).into();
        let problems =
            viewport_problems(short, &renderer(), Size::new(360.0, 640.0), Padding::ZERO);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_word_wider_than_its_box_is_a_problem_and_a_word_in_its_box_is_not() {
        // "abstract" needs more than 40 pixels at this size, so a box of 40 is
        // too narrow for it: the word is drawn past its box.
        let squeezed: Element<'_, ()> = container(text("abstract"))
            .width(Length::Fixed(40.0))
            .into();
        let problems = viewport_problems(
            squeezed,
            &renderer(),
            Size::new(360.0, 640.0),
            Padding::ZERO,
        );
        assert!(
            problems
                .iter()
                .any(|p| p.contains("\"abstract\"") && p.contains("needs")),
            "{problems:?}"
        );

        let roomy: Element<'_, ()> = container(text("abstract"))
            .width(Length::Fixed(200.0))
            .into();
        let problems =
            viewport_problems(roomy, &renderer(), Size::new(360.0, 640.0), Padding::ZERO);
        assert!(problems.is_empty(), "{problems:?}");
    }
}
