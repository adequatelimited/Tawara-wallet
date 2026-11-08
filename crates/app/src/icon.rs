// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The line icons, drawn from SVG path data on an iced canvas.
//!
//! Each icon is an SVG path in a 24 by 24 box, stroked with round caps and
//! joins and no fill. Path data is parsed here: the commands the icons use
//! (`M L H V C S A Z`, absolute and relative), with each elliptical arc
//! turned into cubic curves, since the canvas draws circular arcs only.

use iced::mouse;
use iced::widget::canvas::{self, Frame, LineCap, LineJoin, Path, Stroke};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme};

/// A line icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Wallet,
    Send,
    Receive,
    Activity,
    Cube,
    Sliders,
    Lock,
    Search,
    Clock,
    Plus,
    Copy,
    Swap,
    FileUpload,
    Scan,
    ChevronDown,
    Check,
    Trash,
    Key,
    Download,
    ShieldCheck,
    Refresh,
    Warning,
    Server,
    ChevronLeft,
    ChevronRight,
    /// Three stacked layers, for staking: Lucide's `layers`, in the same 24
    /// by 24 box and stroke.
    Layers,
}

/// One drawn part of an icon.
#[derive(Clone, Copy, Debug)]
enum Part {
    Path(&'static str),
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
    },
}

impl Icon {
    fn parts(self) -> &'static [Part] {
        use Part::{Circle, Path, Rect};
        match self {
            Icon::Wallet => &[
                Rect {
                    x: 2.0,
                    y: 5.0,
                    w: 20.0,
                    h: 15.0,
                    r: 3.0,
                },
                Path("M2 10h20M16 15h2"),
            ],
            Icon::Send => &[Path("M7 17 17 7M7 7h10v10")],
            Icon::Receive => &[Path("M17 7 7 17M17 17H7V7")],
            Icon::Activity => &[Path("M22 12h-4l-3 9L9 3l-3 9H2")],
            Icon::Cube => &[
                Path("M21 8 12 3 3 8v8l9 5 9-5V8z"),
                Path("m3 8 9 5 9-5M12 13v8"),
            ],
            Icon::Sliders => &[Path(
                "M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6",
            )],
            Icon::Lock => &[
                Rect {
                    x: 3.0,
                    y: 11.0,
                    w: 18.0,
                    h: 11.0,
                    r: 2.0,
                },
                Path("M7 11V7a5 5 0 0 1 10 0v4"),
            ],
            Icon::Search => &[
                Circle {
                    cx: 11.0,
                    cy: 11.0,
                    r: 8.0,
                },
                Path("m21 21-4.3-4.3"),
            ],
            Icon::Clock => &[
                Circle {
                    cx: 12.0,
                    cy: 12.0,
                    r: 10.0,
                },
                Path("M12 6v6l4 2"),
            ],
            Icon::Plus => &[Path("M12 5v14M5 12h14")],
            Icon::Copy => &[
                Rect {
                    x: 9.0,
                    y: 9.0,
                    w: 13.0,
                    h: 13.0,
                    r: 2.0,
                },
                Path("M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"),
            ],
            Icon::Swap => &[Path("M17 3l4 4-4 4M3 7h18M7 21l-4-4 4-4M21 17H3")],
            Icon::FileUpload => &[
                Path("M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"),
                Path("M14 2v6h6M12 18v-6M9 15l3-3 3 3"),
            ],
            Icon::Scan => &[Path(
                "M3 7V5a2 2 0 0 1 2-2h2M17 3h2a2 2 0 0 1 2 2v2M21 17v2a2 2 0 0 1-2 2h-2M7 \
                 21H5a2 2 0 0 1-2-2v-2M7 12h10",
            )],
            Icon::ChevronDown => &[Path("m6 9 6 6 6-6")],
            Icon::Check => &[Path("M20 6 9 17l-5-5")],
            Icon::Trash => &[Path("M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6")],
            Icon::Key => &[
                Circle {
                    cx: 7.5,
                    cy: 15.5,
                    r: 5.5,
                },
                Path("m21 2-9.6 9.6M15.5 7.5l3 3L22 7l-3-3"),
            ],
            Icon::Download => &[Path(
                "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3",
            )],
            Icon::ShieldCheck => &[
                Path("M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"),
                Path("m9 12 2 2 4-4"),
            ],
            Icon::Refresh => &[Path("M21 12a9 9 0 1 1-3-6.7L21 8"), Path("M21 3v5h-5")],
            Icon::Warning => &[
                Path(
                    "M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 \
                     0-3.4 0z",
                ),
                Path("M12 9v4M12 17h.01"),
            ],
            Icon::Server => &[
                Rect {
                    x: 2.0,
                    y: 3.0,
                    w: 20.0,
                    h: 8.0,
                    r: 2.0,
                },
                Rect {
                    x: 2.0,
                    y: 13.0,
                    w: 20.0,
                    h: 8.0,
                    r: 2.0,
                },
                Path("M6 7h.01M6 17h.01"),
            ],
            Icon::ChevronLeft => &[Path("m15 18-6-6 6-6")],
            Icon::ChevronRight => &[Path("m9 18 6-6-6-6")],
            Icon::Layers => &[
                Path("M12 2 2 7l10 5 10-5-10-5z"),
                Path("m2 17 10 5 10-5M2 12l10 5 10-5"),
            ],
        }
    }
}

/// `icon` at `size` pixels, stroked `stroke` units wide in its 24-unit box,
/// in `color`.
pub fn icon<'a, Message: 'a>(
    icon: Icon,
    size: f32,
    stroke: f32,
    color: Color,
) -> Element<'a, Message> {
    canvas(Drawn {
        icon,
        stroke,
        color,
    })
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .into()
}

fn canvas<Message>(program: Drawn) -> canvas::Canvas<Drawn, Message> {
    canvas::Canvas::new(program)
}

struct Drawn {
    icon: Icon,
    stroke: f32,
    color: Color,
}

impl<Message> canvas::Program<Message> for Drawn {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let scale = bounds.width.min(bounds.height) / 24.0;
        let at = |x: f32, y: f32| Point::new(x * scale, y * scale);
        let path = Path::new(|b| {
            for part in self.icon.parts() {
                match *part {
                    Part::Path(data) => {
                        for segment in parse(data).unwrap_or_default() {
                            match segment {
                                Segment::Move(p) => b.move_to(at(p.0, p.1)),
                                Segment::Line(p) => b.line_to(at(p.0, p.1)),
                                Segment::Cubic(c1, c2, p) => {
                                    b.bezier_curve_to(at(c1.0, c1.1), at(c2.0, c2.1), at(p.0, p.1));
                                }
                                Segment::Close => b.close(),
                            }
                        }
                    }
                    Part::Rect { x, y, w, h, r } => {
                        b.rounded_rectangle(
                            at(x, y),
                            iced::Size::new(w * scale, h * scale),
                            iced::border::Radius::from(r * scale),
                        );
                    }
                    Part::Circle { cx, cy, r } => b.circle(at(cx, cy), r * scale),
                }
            }
        });
        frame.stroke(
            &path,
            Stroke::default()
                .with_color(self.color)
                .with_width(self.stroke * scale)
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round),
        );
        vec![frame.into_geometry()]
    }
}

// --------------------------------------------------------------- path data

type Pt = (f32, f32);

/// A path segment in absolute coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Segment {
    Move(Pt),
    Line(Pt),
    Cubic(Pt, Pt, Pt),
    Close,
}

/// The tokens of SVG path data: command letters and numbers. A number ends
/// where a sign, a second decimal point, a letter or a separator begins, so
/// `1-2-2` is three numbers and `.5.5` two.
fn tokens(data: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut number = String::new();
    let flush = |number: &mut String, out: &mut Vec<Token>| {
        if !number.is_empty() {
            out.push(Token::Number(number.parse().unwrap_or(0.0)));
            number.clear();
        }
    };
    for c in data.chars() {
        match c {
            '0'..='9' => number.push(c),
            '.' => {
                if number.contains('.') {
                    flush(&mut number, &mut out);
                }
                number.push(c);
            }
            '-' | '+' => {
                flush(&mut number, &mut out);
                number.push(c);
            }
            c if c.is_ascii_alphabetic() => {
                flush(&mut number, &mut out);
                out.push(Token::Command(c));
            }
            _ => flush(&mut number, &mut out),
        }
    }
    flush(&mut number, &mut out);
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Command(char),
    Number(f32),
}

/// Parse path data into absolute segments, or the index of the token where
/// it stopped making sense. The icons' data is fixed and every icon is
/// tested to parse whole, so a drawn icon never meets the error.
fn parse(data: &str) -> Result<Vec<Segment>, usize> {
    let tokens = tokens(data);
    let mut out = Vec::new();
    let mut i = 0;
    let mut command = 'M';
    let mut current: Pt = (0.0, 0.0);
    let mut start: Pt = (0.0, 0.0);
    // The second control point of the last cubic, for `S`.
    let mut last_control: Option<Pt> = None;
    let number = |i: &mut usize| -> Option<f32> {
        match tokens.get(*i) {
            Some(Token::Number(n)) => {
                *i += 1;
                Some(*n)
            }
            _ => None,
        }
    };
    while i < tokens.len() {
        if let Token::Command(c) = tokens[i] {
            command = c;
            i += 1;
        }
        let relative = command.is_ascii_lowercase();
        let base = if relative { current } else { (0.0, 0.0) };
        let point = |x: f32, y: f32| (base.0 + x, base.1 + y);
        match command.to_ascii_uppercase() {
            'M' => {
                let (Some(x), Some(y)) = (number(&mut i), number(&mut i)) else {
                    return Err(i);
                };
                current = point(x, y);
                start = current;
                out.push(Segment::Move(current));
                // Further pairs after a move are lines.
                command = if relative { 'l' } else { 'L' };
                last_control = None;
            }
            'L' => {
                let (Some(x), Some(y)) = (number(&mut i), number(&mut i)) else {
                    return Err(i);
                };
                current = point(x, y);
                out.push(Segment::Line(current));
                last_control = None;
            }
            'H' => {
                let Some(x) = number(&mut i) else {
                    return Err(i);
                };
                current = (if relative { current.0 + x } else { x }, current.1);
                out.push(Segment::Line(current));
                last_control = None;
            }
            'V' => {
                let Some(y) = number(&mut i) else {
                    return Err(i);
                };
                current = (current.0, if relative { current.1 + y } else { y });
                out.push(Segment::Line(current));
                last_control = None;
            }
            'C' => {
                let n: Vec<f32> = (0..6).filter_map(|_| number(&mut i)).collect();
                let [x1, y1, x2, y2, x, y] = n[..] else {
                    return Err(i);
                };
                let (c1, c2, to) = (point(x1, y1), point(x2, y2), point(x, y));
                out.push(Segment::Cubic(c1, c2, to));
                current = to;
                last_control = Some(c2);
            }
            'S' => {
                let n: Vec<f32> = (0..4).filter_map(|_| number(&mut i)).collect();
                let [x2, y2, x, y] = n[..] else { return Err(i) };
                let c1 = match last_control {
                    Some(c) => (2.0 * current.0 - c.0, 2.0 * current.1 - c.1),
                    None => current,
                };
                let (c2, to) = (point(x2, y2), point(x, y));
                out.push(Segment::Cubic(c1, c2, to));
                current = to;
                last_control = Some(c2);
            }
            'A' => {
                let n: Vec<f32> = (0..7).filter_map(|_| number(&mut i)).collect();
                let [rx, ry, rotation, large, sweep, x, y] = n[..] else {
                    return Err(i);
                };
                let to = point(x, y);
                out.extend(
                    arc(current, to, rx, ry, rotation, large != 0.0, sweep != 0.0)
                        .into_iter()
                        .map(|(c1, c2, p)| Segment::Cubic(c1, c2, p)),
                );
                current = to;
                last_control = None;
            }
            'Z' => {
                out.push(Segment::Close);
                current = start;
                last_control = None;
            }
            _ => return Err(i),
        }
    }
    Ok(out)
}

/// An SVG elliptical arc from `from` to `to`, as cubic curves of at most a
/// quarter turn each (SVG 1.1, appendix F.6.5, endpoint to centre).
fn arc(
    from: Pt,
    to: Pt,
    rx: f32,
    ry: f32,
    rotation: f32,
    large: bool,
    sweep: bool,
) -> Vec<(Pt, Pt, Pt)> {
    if from == to {
        return Vec::new();
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        return vec![(from, to, to)];
    }
    let phi = rotation.to_radians();
    let (sin, cos) = phi.sin_cos();
    let dx = (from.0 - to.0) / 2.0;
    let dy = (from.1 - to.1) / 2.0;
    let x1 = cos * dx + sin * dy;
    let y1 = -sin * dx + cos * dy;
    // Radii too small for the chord are scaled up until they fit.
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (num / den).max(0.0).sqrt();
    if large == sweep {
        k = -k;
    }
    let cx1 = k * rx * y1 / ry;
    let cy1 = -k * ry * x1 / rx;
    let cx = cos * cx1 - sin * cy1 + (from.0 + to.0) / 2.0;
    let cy = sin * cx1 + cos * cy1 + (from.1 + to.1) / 2.0;
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let theta1 = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut delta = angle(
        (x1 - cx1) / rx,
        (y1 - cy1) / ry,
        (-x1 - cx1) / rx,
        (-y1 - cy1) / ry,
    );
    if !sweep && delta > 0.0 {
        delta -= 2.0 * core::f32::consts::PI;
    } else if sweep && delta < 0.0 {
        delta += 2.0 * core::f32::consts::PI;
    }
    let pieces = (delta.abs() / core::f32::consts::FRAC_PI_2).ceil().max(1.0);
    let step = delta / pieces;
    let handle = 4.0 / 3.0 * (step / 4.0).tan();
    // A point on the ellipse at parameter `t`, and its derivative.
    let on = |t: f32| {
        let (s, c) = t.sin_cos();
        (
            cx + rx * c * cos - ry * s * sin,
            cy + rx * c * sin + ry * s * cos,
        )
    };
    let slope = |t: f32| {
        let (s, c) = t.sin_cos();
        (-rx * s * cos - ry * c * sin, -rx * s * sin + ry * c * cos)
    };
    let mut out = Vec::new();
    let mut t = theta1;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    for n in 0..pieces as u32 {
        let t2 = t + step;
        let p1 = if n == 0 { from } else { on(t) };
        let p2 = if n + 1 == pieces as u32 { to } else { on(t2) };
        let d1 = slope(t);
        let d2 = slope(t2);
        out.push((
            (p1.0 + handle * d1.0, p1.1 + handle * d1.1),
            (p2.0 - handle * d2.0, p2.1 - handle * d2.1),
            p2,
        ));
        t = t2;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Icon; 26] = [
        Icon::Wallet,
        Icon::Send,
        Icon::Receive,
        Icon::Activity,
        Icon::Cube,
        Icon::Sliders,
        Icon::Lock,
        Icon::Search,
        Icon::Clock,
        Icon::Plus,
        Icon::Copy,
        Icon::Swap,
        Icon::FileUpload,
        Icon::Scan,
        Icon::ChevronDown,
        Icon::Check,
        Icon::Trash,
        Icon::Key,
        Icon::Download,
        Icon::ShieldCheck,
        Icon::Refresh,
        Icon::Warning,
        Icon::Server,
        Icon::ChevronLeft,
        Icon::ChevronRight,
        Icon::Layers,
    ];

    #[test]
    fn numbers_split_on_signs_and_second_points() {
        assert_eq!(
            tokens("m21 21-4.3-4.3"),
            vec![
                Token::Command('m'),
                Token::Number(21.0),
                Token::Number(21.0),
                Token::Number(-4.3),
                Token::Number(-4.3),
            ]
        );
        assert_eq!(
            tokens("h.01.5"),
            vec![Token::Command('h'), Token::Number(0.01), Token::Number(0.5)]
        );
    }

    #[test]
    fn a_move_followed_by_pairs_draws_lines() {
        assert_eq!(
            parse("M7 17 17 7M7 7h10v10").unwrap(),
            vec![
                Segment::Move((7.0, 17.0)),
                Segment::Line((17.0, 7.0)),
                Segment::Move((7.0, 7.0)),
                Segment::Line((17.0, 7.0)),
                Segment::Line((17.0, 17.0)),
            ]
        );
    }

    #[test]
    fn relative_commands_follow_the_current_point() {
        assert_eq!(
            parse("m3 8 9 5 9-5").unwrap(),
            vec![
                Segment::Move((3.0, 8.0)),
                Segment::Line((12.0, 13.0)),
                Segment::Line((21.0, 8.0)),
            ]
        );
    }

    #[test]
    fn an_arc_ends_where_it_says_and_stays_on_its_circle() {
        // The lock's shackle: a half circle of radius 5 from (7, 7) to (17, 7)
        // over the top, centred on (12, 7).
        let segments = parse("M7 7a5 5 0 0 1 10 0").unwrap();
        let Some(Segment::Cubic(_, _, end)) = segments.last() else {
            panic!("{segments:?}")
        };
        assert!(
            (end.0 - 17.0).abs() < 1e-4 && (end.1 - 7.0).abs() < 1e-4,
            "{end:?}"
        );
        assert_eq!(segments.len(), 3, "a half turn is two quarter turns");
        let Segment::Cubic(_, _, mid) = segments[1] else {
            panic!()
        };
        assert!(
            (mid.0 - 12.0).abs() < 1e-3 && (mid.1 - 2.0).abs() < 1e-3,
            "{mid:?}"
        );
    }

    #[test]
    fn a_smooth_cubic_reflects_the_last_control_point() {
        let segments = parse("M12 22s8-4 8-10").unwrap();
        assert_eq!(
            segments[1],
            Segment::Cubic((12.0, 22.0), (20.0, 18.0), (20.0, 12.0))
        );
    }

    #[test]
    fn every_icon_parses_whole_and_stays_in_its_box() {
        for icon in ALL {
            for part in icon.parts() {
                if let Part::Path(data) = part {
                    let segments = parse(data)
                        .unwrap_or_else(|at| panic!("{icon:?}: {data} stops at token {at}"));
                    let points = segments.iter().flat_map(|s| match s {
                        Segment::Move(p) | Segment::Line(p) => vec![*p],
                        Segment::Cubic(a, b, c) => vec![*a, *b, *c],
                        Segment::Close => vec![],
                    });
                    for (x, y) in points {
                        assert!(
                            (-1.0..=25.0).contains(&x) && (-1.0..=25.0).contains(&y),
                            "{icon:?}: ({x}, {y}) is outside its box"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn data_that_makes_no_sense_is_refused() {
        assert!(parse("M1").is_err());
        assert!(parse("Q1 2 3 4").is_err());
    }
}
