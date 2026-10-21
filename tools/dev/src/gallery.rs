//! Contact sheets: every screen a plugin adds and every widget in every
//! size it can take, on every device profile, in light and dark, in each
//! language and look, as PNG pictures and HTML pages to look at quickly.
//!
//! What it writes, under `<out>/gallery/`:
//!
//! - `index.html`: per plugin, its screens and widgets, each linked to its
//!   page, and its PNG sheets.
//! - `<plugin>/screens/<screen>.html`: the screen in every variant side by
//!   side, a row per device profile; the pictures in
//!   `<plugin>/screens/<screen>/<variant>.png`.
//! - `<plugin>/widgets/<widget>.html`: per variant the widget in every size,
//!   laid out like the home grid (a line per height, widths growing to the
//!   right, so a 2x1 is twice as wide as a 1x1); the pictures (the cell cut
//!   out) in `<plugin>/widgets/<widget>/<variant>/<columns>x<rows>.png`, and
//!   the whole home screen with it in one size for context in `screen.png`.
//! - `<plugin>/<profile>.png`: one contact sheet per plugin and profile (see
//!   `pages` for its fixed layout; a large one goes on in
//!   `<profile>-2.png` and so on).
//!
//! Widgets are shown in every size in the first variant of each profile
//! and language, in a few sizes (`cases::some_sizes`) in the others, and
//! in a few sizes only with `--quick`; see `every_size`.
//!
//! A picture the same as one written before (a widget that looks the same
//! in two looks, say) is written once; the pages link to the first.
//!
//! A screen that does not open without parameters is tried once with a
//! few common ones (a book, the books folder, a word, a text); if none
//! makes it open it is left out, with a note.

mod pages;

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use paperpaw_core::sdk::builtin::settings::NOTICE_SECONDS;

use crate::cli::Context;
use crate::image::Image;
use crate::report::{Finding, Report};
use crate::session::{self, Session};
use crate::target::Target;
use crate::variant::Variant;
use crate::{cases, probe};

/// Parameters tried, in order, for a screen that does not open without:
/// those many screens take, with values the sample library has.
const PARAMS: &[&[(&str, &str)]] = &[
    &[],
    &[("path", session::BOOK)],
    &[("path", "books")],
    &[("word", "rabbit"), ("language", "en")],
    &[("text", "Sample text"), ("title", "Sample")],
];

/// A picture written, relative to the gallery folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pic {
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// A screen in the variants.
#[derive(Debug, Default)]
pub struct ScreenCase {
    /// The parameters it was opened with (an index into `PARAMS`),
    /// `None` if it opened with none of them.
    pub params: Option<usize>,
    /// Per variant (by index), the picture if it opened.
    pub shots: BTreeMap<usize, Pic>,
}

/// A widget in one size and variant.
#[derive(Debug, Clone)]
pub struct WidgetShot {
    pub variant: usize,
    pub size: (u8, u8),
    /// `None` if the grid did not place it in this size.
    pub pic: Option<Pic>,
    /// Whether it drew nothing in its cell.
    pub empty: bool,
}

/// A widget in the variants.
#[derive(Debug, Default)]
pub struct WidgetCase {
    pub title: String,
    pub shots: Vec<WidgetShot>,
    /// Per variant, the whole home screen with it, in one size.
    pub context: BTreeMap<usize, Pic>,
}

/// What one plugin adds, shown.
#[derive(Debug, Default)]
pub struct PluginCases {
    pub screens: BTreeMap<String, ScreenCase>,
    pub widgets: BTreeMap<String, WidgetCase>,
}

/// Everything shown in a run.
#[derive(Debug, Default)]
pub struct Gallery {
    pub variants: Vec<Variant>,
    pub plugins: BTreeMap<String, PluginCases>,
    /// The sizes looked at, in order (rows, then columns).
    pub sizes: Vec<(u8, u8)>,
    /// Whether only a few sizes were looked at everywhere (`--quick`).
    pub quick: bool,
    /// Per variant, the cell of each size (the same for every widget).
    pub cells: HashMap<(usize, (u8, u8)), (u32, u32)>,
    /// The folder written into.
    pub dir: PathBuf,
    /// Pictures written, by content.
    written: HashMap<u64, Pic>,
    /// Pictures shown and files written.
    pub shown: usize,
    pub files: usize,
}

impl Gallery {
    /// Writes `image` to `path` (relative to the gallery folder) unless the
    /// same picture was written before; the picture to link to.
    fn save(&mut self, image: &Image, path: String) -> Result<Pic, String> {
        self.shown += 1;
        let mut hasher = std::hash::DefaultHasher::new();
        (image.width, image.height, &image.rgb).hash(&mut hasher);
        let key = hasher.finish();
        if let Some(pic) = self.written.get(&key) {
            return Ok(pic.clone());
        }
        save_png(image, &self.dir.join(&path))?;
        self.files += 1;
        let pic = Pic {
            path,
            width: image.width,
            height: image.height,
        };
        self.written.insert(key, pic.clone());
        Ok(pic)
    }

    fn plugin(&mut self, plugin: &str) -> &mut PluginCases {
        self.plugins.entry(plugin.to_string()).or_default()
    }

    /// The variants of `profile`, by index, in order.
    pub fn of_profile(&self, profile: &str) -> Vec<usize> {
        (0..self.variants.len())
            .filter(|&i| self.variants[i].profile.device.id == profile)
            .collect()
    }

    /// The profiles shown, in order.
    pub fn profiles(&self) -> Vec<&'static str> {
        let mut profiles: Vec<&'static str> = Vec::new();
        for variant in &self.variants {
            if !profiles.contains(&variant.profile.device.id) {
                profiles.push(variant.profile.device.id);
            }
        }
        profiles
    }
}

/// The plugin `id` belongs to: the one looked at whose namespace it is
/// in, else its namespace.
fn plugin_of(target: &Target, id: &str) -> String {
    target
        .owner(id)
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            id.split(['.', '@', ' ', '/'])
                .next()
                .unwrap_or(id)
                .to_string()
        })
}

/// The size a widget is also shown in on the whole screen, for context.
fn context_size(grid: (u8, u8)) -> (u8, u8) {
    (grid.0.min(2), grid.1.min(2))
}

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let started = paperpaw_emulator::host::micros();
    let dir = context.path("gallery");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let gallery = make(context, report, dir)?;
    pages::write(&gallery)?;
    report.page("Gallery: every screen and widget", "gallery/index.html");
    for plugin in gallery.plugins.keys() {
        report.page(
            format!("Gallery of {plugin}"),
            format!("gallery/index.html#{plugin}"),
        );
    }
    let message = format!(
        "{} pictures in {} variants ({} files) in {:.1} s.",
        gallery.shown,
        gallery.variants.len(),
        gallery.files,
        (paperpaw_emulator::host::micros() - started) as f64 / 1e6
    );
    eprintln!("paperpaw-dev: gallery: {message}");
    report.add(Finding::info("gallery.made", "gallery/index.html", message));
    Ok(())
}

/// Shows every case in every variant, writing the pictures.
pub fn make(context: &Context, report: &mut Report, dir: PathBuf) -> Result<Gallery, String> {
    let mut gallery = Gallery {
        variants: context.variants(),
        quick: context.quick,
        dir,
        ..Gallery::default()
    };
    let mut tried: HashMap<String, Option<usize>> = HashMap::new();
    for index in 0..gallery.variants.len() {
        let variant = gallery.variants[index].clone();
        if context.verbose {
            eprintln!("paperpaw-dev: gallery: {}", variant.name());
        }
        let mut session = Session::new(context, &variant)?;
        let home = session.screen();
        for id in cases::screens(&session, &context.target) {
            let plugin = plugin_of(&context.target, &id);
            let shot = screen(&mut session, &id, &mut tried, report, &plugin)?;
            let params = tried.get(&id).copied().flatten();
            let case = gallery
                .plugin(&plugin)
                .screens
                .entry(id.clone())
                .or_default();
            case.params = params;
            if let Some(image) = shot {
                let path = format!("{plugin}/screens/{id}/{}.png", variant.slug());
                let pic = gallery.save(&image, path)?;
                gallery
                    .plugin(&plugin)
                    .screens
                    .get_mut(&id)
                    .map(|c| c.shots.insert(index, pic));
            }
            // Back home for the next; a screen that keeps the device
            // (asleep, locked) is left with a fresh session.
            cases::home(&mut session);
            quiet(&mut session);
            if session.screen() != home {
                session = Session::new(context, &variant)?;
            }
        }
        widgets(context, report, &mut gallery, &mut session, index)?;
    }
    Ok(gallery)
}

/// Opens a screen, with the parameters that made it open in an earlier
/// variant or, the first time, the first of [`PARAMS`] that does.
fn screen(
    session: &mut Session,
    id: &str,
    tried: &mut HashMap<String, Option<usize>>,
    report: &mut Report,
    plugin: &str,
) -> Result<Option<Image>, String> {
    let variant = session.variant.name();
    if let Some(known) = tried.get(id) {
        let Some(params) = *known else {
            return Ok(None);
        };
        let shown = cases::show_screen(session, id, PARAMS[params])?;
        if !shown.opened() {
            report.add(
                Finding::info(
                    "gallery.not-opened",
                    id,
                    "Did not open in this variant, though it did in others, so it is not shown here.",
                )
                .plugin(plugin)
                .variant(&variant),
            );
            return Ok(None);
        }
        return Ok(Some(shown.image));
    }
    for (index, params) in PARAMS.iter().enumerate() {
        if index > 0 {
            cases::home(session);
            quiet(session);
        }
        let shown = cases::show_screen(session, id, params)?;
        if shown.opened() {
            tried.insert(id.to_string(), Some(index));
            if index > 0 {
                let given: Vec<String> = params.iter().map(|(k, v)| format!("{k} = {v}")).collect();
                report.add(
                    Finding::info(
                        "gallery.needs-params",
                        id,
                        format!(
                            "Does not open without parameters; shown with {}.",
                            given.join(", ")
                        ),
                    )
                    .plugin(plugin),
                );
            }
            return Ok(Some(shown.image));
        }
    }
    tried.insert(id.to_string(), None);
    report.add(
        Finding::info(
            "gallery.needs-params",
            id,
            "Does not open without parameters (nor with a book, a folder, a word or a text), so the gallery leaves it out.",
        )
        .plugin(plugin),
    );
    Ok(None)
}

/// Shows every widget of the target in every size in one variant.
fn widgets(
    context: &Context,
    report: &mut Report,
    gallery: &mut Gallery,
    session: &mut Session,
    index: usize,
) -> Result<(), String> {
    let slug = session.variant.slug();
    let variant = session.variant.name();
    let grid = cases::grid(session);
    let sizes = if every_size(context, &gallery.variants, index) {
        cases::sizes(grid)
    } else {
        cases::some_sizes(grid)
    };
    for &size in &sizes {
        if !gallery.sizes.contains(&size) {
            gallery.sizes.push(size);
        }
    }
    gallery.sizes.sort_by_key(|&(w, h)| (h, w));
    let around = context_size(grid);
    // The grid gives every widget of a size the same cell: measured once
    // a size with the probe, then each widget drawn in it (one frame each,
    // rather than the two of `cases::show_widget`).
    cases::home(session);
    quiet(session);
    let mut cells = HashMap::new();
    for &size in &sizes {
        probe::forget();
        session.set(cases::PAGE, &place(probe::WIDGET.as_str(), size))?;
        if let Some(cell) = probe::area() {
            cells.insert(size, cell);
            gallery.cells.insert(
                (index, size),
                (u32::from(cell.width), u32::from(cell.height)),
            );
        }
    }
    for (id, title) in cases::widgets(session, &context.target) {
        let plugin = plugin_of(&context.target, &id);
        gallery
            .plugin(&plugin)
            .widgets
            .entry(id.clone())
            .or_default()
            .title = title;
        for &size in &sizes {
            let named = format!("{id} {}x{}", size.0, size.1);
            let mut shot = WidgetShot {
                variant: index,
                size,
                pic: None,
                empty: false,
            };
            match cells.get(&size) {
                None => report.add(
                    Finding::info(
                        "gallery.not-placed",
                        &named,
                        "The home grid did not place it in this size, so it is not shown.",
                    )
                    .plugin(&plugin)
                    .variant(&variant),
                ),
                Some(&cell) => {
                    session.set(cases::PAGE, &place(&id, size))?;
                    let screen = session.image();
                    let picture = screen.crop(cell);
                    shot.empty = uniform(&picture);
                    if shot.empty {
                        report.add(
                            Finding::info(
                                "gallery.widget-empty",
                                &named,
                                "Draws nothing in this size; if it cannot show anything useful here, it could say so or refuse the size.",
                            )
                            .plugin(&plugin)
                            .variant(&variant),
                        );
                    }
                    let base = format!("{plugin}/widgets/{id}/{slug}");
                    shot.pic =
                        Some(gallery.save(&picture, format!("{base}/{}x{}.png", size.0, size.1))?);
                    if size == around {
                        let pic = gallery.save(&screen, format!("{base}/screen.png"))?;
                        if let Some(case) = gallery.plugin(&plugin).widgets.get_mut(&id) {
                            case.context.insert(index, pic);
                        }
                    }
                }
            }
            if let Some(case) = gallery.plugin(&plugin).widgets.get_mut(&id) {
                case.shots.push(shot);
            }
        }
    }
    Ok(())
}

/// Lets a notice a case left (e.g. "no Wi-Fi") run out, so that it does
/// not cover the next case: emulated time moves on past its end.
fn quiet(session: &mut Session) {
    let seconds = session
        .device
        .kernel
        .settings()
        .text(NOTICE_SECONDS.as_str())
        .parse::<u64>()
        .unwrap_or(4)
        .clamp(1, 60);
    session.device.advance(seconds * 1000 + 500);
    session.device.tick_if_due();
    session.settle();
}

/// Writes `image` as PNG, compressed fast: a gallery writes many large
/// pictures that are mostly one colour, which compress well anyway.
fn save_png(image: &Image, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writer
        .write_image_data(&image.rgb)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether widgets are shown in every size in variant `index`: in the
/// first variant of each profile and language (light and the default
/// look, unless those were left out), where a size too small for the
/// text shows; in the others (dark, the other looks, where what goes
/// wrong rarely depends on the size) in a few sizes, which keeps a plugin
/// with many widgets to minutes. With `--quick` (one variant a profile)
/// in a few sizes only.
fn every_size(context: &Context, variants: &[Variant], index: usize) -> bool {
    let v = &variants[index];
    !context.quick
        && variants[..index]
            .iter()
            .all(|o| o.profile.device.id != v.profile.device.id || o.language != v.language)
}

/// The home page's setting for `widget` alone at the top left in `size`.
fn place(widget: &str, size: (u8, u8)) -> String {
    format!("{widget} 0/0 {}x{}", size.0, size.1)
}

/// Whether every pixel of `image` is the same: nothing drawn.
fn uniform(image: &Image) -> bool {
    let (pixels, _) = image.rgb.as_chunks::<3>();
    match pixels.split_first() {
        Some((first, rest)) => rest.iter().all(|p| p == first),
        None => true,
    }
}

/// `path` relative to the gallery folder, as a link from a page in
/// `from` (also relative to it).
fn link(from: &str, path: &str) -> String {
    let depth = Path::new(from).components().count().saturating_sub(1);
    format!("{}{path}", "../".repeat(depth))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_climb_out_of_the_page_folder() {
        assert_eq!(link("index.html", "a/b.png"), "a/b.png");
        assert_eq!(
            link("widgets/screens/widgets.x.html", "widgets/x.png"),
            "../../widgets/x.png"
        );
    }

    #[test]
    fn an_empty_cell_is_uniform() {
        let mut image = Image::filled(4, 3, [250, 250, 250]);
        assert!(uniform(&image));
        image.set_pixel(1, 1, [0, 0, 0]);
        assert!(!uniform(&image));
    }
}
