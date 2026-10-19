//! Weather symbols drawn from shapes (discs, round-ended lines), so that
//! they scale to any size and stay clean on panels with two colours:
//! the sun, the moon, clouds, rain, snow, lightning and fog.
//!
//! A symbol is a few layers, the front one first; a cloud before the sun
//! hides what lies behind it with a little room around it. Its shapes are
//! laid out in a square 1000 units wide and tested a pixel at a time in
//! whole numbers (small devices have no floating-point unit), four
//! samples a pixel for smooth edges where the panel has grays. Only ink
//! is drawn, so the symbol sits on any frame, light or dark.

use alloc::vec::Vec;

use quireink_sdk::round::sin_cos;
use quireink_sdk::{Canvas, Color, Rect};

use crate::data::Symbol;

/// The size of the square the shapes are laid out in.
const UNITS: i32 = 1000;

/// How symbols are drawn: in outlines, or filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Style {
    Line,
    Solid,
}

/// A shape: a disc, or a line with round ends (`w` half its width).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Disc {
        x: i32,
        y: i32,
        r: i32,
    },
    Line {
        ax: i32,
        ay: i32,
        bx: i32,
        by: i32,
        w: i32,
    },
}

impl Shape {
    /// Whether the point `x`, `y` lies in the shape grown by `grow` units
    /// (smaller where it is negative).
    fn holds(self, x: i32, y: i32, grow: i32) -> bool {
        match self {
            Shape::Disc { x: cx, y: cy, r } => {
                let reach = r + grow;
                // Most points lie far off: told apart without multiplying.
                if (x - cx).abs() > reach || (y - cy).abs() > reach {
                    return false;
                }
                let r = i64::from(reach.max(0));
                let (dx, dy) = (i64::from(x - cx), i64::from(y - cy));
                dx * dx + dy * dy <= r * r
            }
            Shape::Line { ax, ay, bx, by, w } => {
                let reach = w + grow;
                if x < ax.min(bx) - reach
                    || x > ax.max(bx) + reach
                    || y < ay.min(by) - reach
                    || y > ay.max(by) + reach
                {
                    return false;
                }
                let w = i64::from(reach.max(0));
                let (px, py) = (i64::from(x - ax), i64::from(y - ay));
                let (vx, vy) = (i64::from(bx - ax), i64::from(by - ay));
                let along = px * vx + py * vy;
                let length = vx * vx + vy * vy;
                let distance = if along <= 0 || length == 0 {
                    px * px + py * py
                } else if along >= length {
                    let (qx, qy) = (i64::from(x - bx), i64::from(y - by));
                    qx * qx + qy * qy
                } else {
                    px * px + py * py - along * along / length
                };
                distance <= w * w
            }
        }
    }
}

/// A part of a symbol: shapes together, less some (`cut`).
#[derive(Debug, Clone, Default)]
struct Layer {
    shapes: Vec<Shape>,
    cut: Vec<Shape>,
    /// Only its outline is drawn, `thickness` wide.
    outline: bool,
    /// It hides the layers behind it, with this much room around it.
    hides: Option<i32>,
}

impl Layer {
    fn filled(shapes: Vec<Shape>) -> Self {
        Self {
            shapes,
            ..Self::default()
        }
    }

    fn holds(&self, x: i32, y: i32, grow: i32) -> bool {
        self.shapes.iter().any(|s| s.holds(x, y, grow))
            && !self.cut.iter().any(|s| s.holds(x, y, -grow))
    }

    fn ink(&self, x: i32, y: i32, thickness: i32) -> bool {
        self.holds(x, y, 0) && !(self.outline && self.holds(x, y, -thickness))
    }
}

/// A symbol as layers, the front one first.
#[derive(Debug, Clone, Default)]
struct Scene {
    layers: Vec<Layer>,
    thickness: i32,
}

impl Scene {
    fn ink(&self, x: i32, y: i32) -> bool {
        for layer in &self.layers {
            if layer.ink(x, y, self.thickness) {
                return true;
            }
            if let Some(room) = layer.hides
                && layer.holds(x, y, room)
            {
                return false;
            }
        }
        false
    }
}

/// The parts of a cloud `w` units wide whose top left is at `x`, `y`;
/// it is about 0.62 `w` high.
fn cloud(x: i32, y: i32, w: i32) -> Vec<Shape> {
    let base = w / 5;
    let bottom = y + w * 62 / 100 - base;
    alloc::vec![
        Shape::Line {
            ax: x + base,
            ay: bottom,
            bx: x + w - base,
            by: bottom,
            w: base,
        },
        Shape::Disc {
            x: x + w * 58 / 100,
            y: y + w * 30 / 100,
            r: w * 30 / 100,
        },
        Shape::Disc {
            x: x + w * 30 / 100,
            y: y + w * 41 / 100,
            r: w / 5,
        },
    ]
}

/// A line from `a` to `b`, `w` units wide.
fn line((ax, ay): (i32, i32), (bx, by): (i32, i32), w: i32) -> Shape {
    Shape::Line {
        ax,
        ay,
        bx,
        by,
        w: (w / 2).max(1),
    }
}

/// The point `r` units from `x`, `y` towards `degrees` (clockwise from
/// twelve).
fn toward(x: i32, y: i32, r: i32, degrees: i32) -> (i32, i32) {
    let (sin, cos) = sin_cos(degrees);
    (x + r * sin / 10_000, y - r * cos / 10_000)
}

struct Builder {
    style: Style,
    thickness: i32,
    scene: Scene,
}

impl Builder {
    fn push(&mut self, layer: Layer) {
        self.scene.layers.push(layer);
    }

    /// A shape filled, or outlined in the line style.
    fn body(&mut self, shapes: Vec<Shape>, cut: Vec<Shape>, hides: bool) {
        self.push(Layer {
            shapes,
            cut,
            outline: self.style == Style::Line,
            hides: hides.then_some(self.thickness * 3 / 4),
        });
    }

    /// A cloud before what follows.
    fn cloud(&mut self, x: i32, y: i32, w: i32) {
        self.body(cloud(x, y, w), Vec::new(), true);
    }

    /// The sun: a disc (or ring) `r` units round with eight rays from
    /// `from` to `to` units out.
    fn sun(&mut self, (x, y): (i32, i32), r: i32, from: i32, to: i32) {
        self.body(alloc::vec![Shape::Disc { x, y, r }], Vec::new(), false);
        let rays = (0..8)
            .map(|i| {
                let d = i * 45;
                line(toward(x, y, from, d), toward(x, y, to, d), self.thickness)
            })
            .collect();
        self.push(Layer::filled(rays));
    }

    /// A crescent moon `r` units round, filled in both styles: it is thin.
    fn moon(&mut self, (x, y): (i32, i32), r: i32) {
        self.push(Layer {
            shapes: alloc::vec![Shape::Disc { x, y, r }],
            cut: alloc::vec![Shape::Disc {
                x: x + r * 45 / 100,
                y: y - r * 32 / 100,
                r: r * 80 / 100,
            }],
            outline: false,
            hides: None,
        });
    }

    /// The sun by day, the moon by night, at `at`, `r` its disc's radius.
    fn sky(&mut self, day: bool, at: (i32, i32), r: i32) {
        if day {
            self.sun(at, r, r + r * 45 / 100, r + r * 105 / 100);
        } else {
            self.moon(at, r * 160 / 100);
        }
    }

    /// Rain in slanted lines below a cloud, from `top` to `bottom`.
    fn rain(&mut self, xs: &[i32], top: i32, bottom: i32) {
        let slant = (bottom - top) * 35 / 100;
        let lines = xs
            .iter()
            .map(|&x| line((x, top), (x - slant, bottom), self.thickness))
            .collect();
        self.push(Layer::filled(lines));
    }

    /// Drizzle: short dashes in two rows.
    fn drizzle(&mut self, xs: &[i32], top: i32) {
        let t = self.thickness;
        let mut dashes = Vec::new();
        for &x in xs {
            dashes.push(line((x, top), (x - 30, top + 80), t));
            dashes.push(line((x - 90, top + 150), (x - 120, top + 230), t));
        }
        self.push(Layer::filled(dashes));
    }

    /// Snowflakes: three crossed lines each.
    fn snow(&mut self, flakes: &[(i32, i32)], arm: i32) {
        let t = self.thickness * 85 / 100;
        let mut lines = Vec::new();
        for &(x, y) in flakes {
            for d in [0, 60, 120] {
                lines.push(line(toward(x, y, arm, d), toward(x, y, arm, d + 180), t));
            }
        }
        self.push(Layer::filled(lines));
    }

    /// Lightning: a zigzag.
    fn bolt(&mut self) {
        let t = self.thickness * 12 / 10;
        let points = [(570, 640), (450, 800), (590, 800), (470, 960)];
        let lines = points.windows(2).map(|p| line(p[0], p[1], t)).collect();
        self.push(Layer::filled(lines));
    }
}

/// The scene of `symbol` (by day or night) in `style`, with lines
/// `thickness` units wide.
fn scene(symbol: Symbol, day: bool, style: Style, thickness: i32) -> Scene {
    let mut b = Builder {
        style,
        thickness,
        scene: Scene {
            layers: Vec::new(),
            thickness,
        },
    };
    // The cloud above rain or snow.
    let wet = |b: &mut Builder| b.cloud(100, 90, 800);
    match symbol {
        Symbol::Clear => b.sky(day, (500, 500), if day { 200 } else { 225 }),
        Symbol::FewClouds => {
            b.cloud(70, 560, 560);
            b.sky(day, (590, 420), 175);
        }
        Symbol::PartlyCloudy => {
            b.cloud(60, 420, 720);
            b.sky(day, (640, 350), 150);
        }
        Symbol::Cloudy => {
            b.cloud(60, 340, 800);
            b.cloud(430, 160, 520);
        }
        Symbol::Fog => {
            b.cloud(180, 110, 640);
            let t = thickness;
            b.push(Layer::filled(alloc::vec![
                line((140, 650), (860, 650), t),
                line((230, 780), (770, 780), t),
                line((140, 910), (860, 910), t),
            ]));
        }
        Symbol::Drizzle => {
            wet(&mut b);
            b.drizzle(&[400, 590, 780], 680);
        }
        Symbol::Rain => {
            wet(&mut b);
            b.rain(&[370, 560, 750], 690, 900);
        }
        Symbol::HeavyRain => {
            wet(&mut b);
            b.rain(&[310, 470, 630, 790], 680, 950);
        }
        Symbol::Sleet => {
            wet(&mut b);
            b.rain(&[360, 780], 690, 900);
            b.snow(&[(530, 820)], 80);
        }
        Symbol::Snow => {
            wet(&mut b);
            b.snow(&[(320, 760), (540, 870), (750, 760)], 80);
        }
        Symbol::Showers | Symbol::SnowShowers => {
            b.cloud(60, 220, 760);
            if symbol == Symbol::Showers {
                b.rain(&[330, 520, 710], 740, 940);
            } else {
                b.snow(&[(300, 800), (520, 880), (720, 790)], 75);
            }
            b.sky(day, (700, 270), 110);
        }
        Symbol::Thunder | Symbol::Hail => {
            wet(&mut b);
            b.bolt();
            if symbol == Symbol::Hail {
                let r = thickness * 7 / 10;
                b.push(Layer::filled(alloc::vec![
                    Shape::Disc { x: 290, y: 740, r },
                    Shape::Disc { x: 320, y: 890, r },
                    Shape::Disc { x: 760, y: 750, r },
                    Shape::Disc { x: 730, y: 900, r },
                ]));
            }
        }
    }
    b.scene
}

/// How wide the symbol's lines are, in units, for a symbol `size` pixels
/// large: about a sixteenth of it, never thinner than two pixels.
fn thickness(size: u32) -> i32 {
    let two_pixels = 2 * UNITS / size.max(1) as i32;
    60.max(two_pixels)
}

/// Draws `symbol` (by day or night) into the square `area`, in `color`.
pub(crate) fn draw(
    canvas: &mut Canvas<'_>,
    area: Rect,
    symbol: Symbol,
    day: bool,
    style: Style,
    color: Color,
) {
    let size = u32::from(area.width.min(area.height));
    if size < 4 {
        return;
    }
    let scene = scene(symbol, day, style, thickness(size));
    let x = i32::from(area.x) + (i32::from(area.width) - size as i32) / 2;
    let y = i32::from(area.y) + (i32::from(area.height) - size as i32) / 2;
    let span = 4 * size as i32;
    let unit = |p: u32, sample: i32| (4 * p as i32 + sample) * UNITS / span;
    canvas.draw_mask(x, y, size, size, color, |px, py| {
        let mut inside = 0u32;
        for (sx, sy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
            if scene.ink(unit(px, sx), unit(py, sy)) {
                inside += 1;
            }
        }
        (inside * 255 / 4) as u8
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_hold_their_points() {
        let disc = Shape::Disc {
            x: 500,
            y: 500,
            r: 100,
        };
        assert!(disc.holds(500, 500, 0) && disc.holds(600, 500, 0));
        assert!(!disc.holds(601, 500, 0) && disc.holds(601, 500, 5));
        assert!(!disc.holds(590, 500, -20));
        let line = Shape::Line {
            ax: 100,
            ay: 100,
            bx: 300,
            by: 100,
            w: 10,
        };
        assert!(line.holds(200, 105, 0) && line.holds(95, 100, 0));
        assert!(!line.holds(200, 115, 0) && !line.holds(315, 100, 0));
    }

    #[test]
    fn a_cloud_hides_the_sun_behind_it() {
        let solid = scene(Symbol::PartlyCloudy, true, Style::Solid, 60);
        // The sun's middle shows; where the cloud lies before it, the cloud.
        assert!(solid.ink(640, 340));
        assert!(solid.ink(400, 700));
        // Outlined, the cloud is empty inside and hides the sun there.
        let lines = scene_lines(Symbol::PartlyCloudy);
        assert!(!lines.ink(420, 700));
        assert!(!lines.ink(640, 340));
        // The night shows the moon instead.
        let night = scene(Symbol::Clear, false, Style::Line, 60);
        assert!(night.ink(400, 600) && !night.ink(700, 300));
    }

    fn scene_lines(symbol: Symbol) -> Scene {
        scene(symbol, true, Style::Line, 60)
    }

    #[test]
    fn every_symbol_has_ink() {
        use Symbol::*;
        for symbol in [
            Clear,
            FewClouds,
            PartlyCloudy,
            Cloudy,
            Fog,
            Drizzle,
            Rain,
            HeavyRain,
            Sleet,
            Snow,
            Showers,
            SnowShowers,
            Thunder,
            Hail,
        ] {
            for style in [Style::Line, Style::Solid] {
                let drawn = scene(symbol, true, style, 60);
                let inked = (0..50)
                    .flat_map(|y| (0..50).map(move |x| (x * 20, y * 20)))
                    .filter(|&(x, y)| drawn.ink(x, y))
                    .count();
                assert!(inked > 40, "{symbol:?} {style:?}: {inked}");
                // Nothing reaches out of the square.
                for at in [-5, 1005] {
                    let out = (0..100).any(|i| drawn.ink(at, i * 10) || drawn.ink(i * 10, at));
                    assert!(!out, "{symbol:?} {style:?} reaches out");
                }
            }
        }
    }

    #[test]
    fn lines_are_never_thinner_than_two_pixels() {
        assert_eq!(thickness(300), 60);
        assert_eq!(thickness(20), 100);
    }
}
