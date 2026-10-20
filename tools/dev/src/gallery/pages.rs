//! The gallery's pages: the HTML pages, and a PNG contact sheet per
//! plugin and profile.
//!
//! A contact sheet (`<plugin>/<profile>.png`) is laid out the same way
//! every time, so that it can be read without the pages:
//!
//! - A light grey background; every picture framed by a 1 pixel grey line
//!   (so white cells show), 8 pixels apart.
//! - One column per variant of the profile, in the order the tool lists
//!   variants: language, then light before dark, then the looks (default,
//!   square, round-strong).
//! - First a row per screen (sorted by id) that opened in one of them,
//!   then, after a wider gap, a row per widget (sorted by id): in each
//!   column the widget in every size, a line per height (one row high
//!   first), widths growing to the right, each picture as large as its
//!   cell, so that a 2x1 is twice as wide as a 1x1. A size the grid did
//!   not place leaves its place empty.
//! - Pictures at their size on the device. A sheet larger than [`BUDGET`]
//!   pixels goes on in further parts (`<profile>-2.png`, ...), the rows in
//!   the same order; only if a single row is larger than that are all its
//!   pictures shrunk by the same whole factor (the index page says which).

use std::collections::HashMap;
use std::fmt::Write as _;

use super::{Gallery, Pic, PluginCases, link};
use crate::image::Image;
use crate::report::{self, escape};

/// The most pixels a part of a contact sheet has.
pub const BUDGET: u64 = 32_000_000;
/// Between pictures, and between the screens and the widgets.
const GAP: u32 = 8;
const SECTION: u32 = 32;
/// Between the sizes of one widget.
const INNER: u32 = 4;
const BACKGROUND: [u8; 3] = [232, 232, 232];
const FRAME: [u8; 3] = [150, 150, 150];

/// Where part `n` (from 0) of the contact sheet of `plugin` on `profile`
/// goes, in the gallery.
pub fn sheet_path(plugin: &str, profile: &str, n: usize) -> String {
    if n == 0 {
        format!("{plugin}/{profile}.png")
    } else {
        format!("{plugin}/{profile}-{}.png", n + 1)
    }
}

fn screen_page(plugin: &str, id: &str) -> String {
    format!("{plugin}/screens/{id}.html")
}

fn widget_page(plugin: &str, id: &str) -> String {
    format!("{plugin}/widgets/{id}.html")
}

/// Writes every page and contact sheet.
pub fn write(gallery: &Gallery) -> Result<(), String> {
    let mut sheets_made: Sheets<'_> = HashMap::new();
    for (plugin, cases) in &gallery.plugins {
        for profile in gallery.profiles() {
            let made = sheets(gallery, plugin, cases, profile)?;
            if !made.0.is_empty() {
                sheets_made.insert((plugin.clone(), profile), made);
            }
        }
        for id in cases.screens.keys() {
            let path = screen_page(plugin, id);
            save(gallery, &path, &screen_html(gallery, plugin, id, &path))?;
        }
        for id in cases.widgets.keys() {
            let path = widget_page(plugin, id);
            save(gallery, &path, &widget_html(gallery, plugin, id, &path))?;
        }
    }
    save(gallery, "index.html", &index_html(gallery, &sheets_made))
}

/// The contact sheets written, per plugin and profile: their parts and
/// the factor their pictures were shrunk by.
type Sheets<'a> = HashMap<(String, &'a str), (Vec<String>, u32)>;

fn save(gallery: &Gallery, path: &str, text: &str) -> Result<(), String> {
    let file = gallery.dir.join(path);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))
}

/// Styles of the gallery's pages, besides the report's.
const STYLE: &str = "<style>\
.scale{margin:8px 0}.scale button{font:inherit;margin-right:4px;padding:2px 10px;border:1px solid #aaa;background:#fff;border-radius:4px;cursor:pointer}\
.scale button.on{background:#333;color:#fff}\
.lines{display:flex;flex-direction:column;gap:4px}.line{display:flex;gap:4px;align-items:flex-start}\
.slot{display:flex;flex-direction:column;font-size:11px;color:#666}\
.slot.empty img{outline:2px dashed #d80}.ph{border:1px dashed #bbb;box-sizing:border-box;color:#999;font-size:11px}\
.none{color:#999;font-size:13px}.notes{color:#666;font-size:13px}\
@media(prefers-color-scheme:dark){a{color:#8ab4f8}.scale button{background:#333;color:#ddd;border-color:#555}.scale button.on{background:#ddd;color:#111}}\
</style>";

/// Sets the pictures' scale from the buttons (pictures carry their size).
const SCALE_SCRIPT: &str = "const scale = s => {\
  for (const e of document.querySelectorAll('[data-w]')) { e.style.width = e.dataset.w * s + 'px'; e.style.height = e.dataset.h * s + 'px'; }\
  for (const b of document.querySelectorAll('[data-scale]')) b.classList.toggle('on', +b.dataset.scale === s); };\
for (const b of document.querySelectorAll('[data-scale]')) b.addEventListener('click', () => scale(+b.dataset.scale));\
scale(+document.querySelector('.scale').dataset.default);";

fn scale_buttons(default: &str) -> String {
    let mut out = format!("<div class=scale data-default={default}>Scale: ");
    for s in ["0.25", "0.5", "1", "2"] {
        let _ = write!(out, "<button data-scale={s}>{s}</button>");
    }
    out.push_str("</div>");
    out
}

fn img(from: &str, pic: &Pic, title: &str) -> String {
    format!(
        "<img src=\"{}\" data-w={} data-h={} title=\"{}\" loading=lazy>",
        escape(&link(from, &pic.path)),
        pic.width,
        pic.height,
        escape(title)
    )
}

/// A small picture for the index, its size given so the page does not
/// jump while pictures load.
fn thumb(pic: &Pic, title: &str) -> String {
    let width = 120;
    let height = pic.height * width / pic.width.max(1);
    format!(
        "<img src=\"{}\" width={width} height={height} title=\"{}\" loading=lazy>",
        escape(&pic.path),
        escape(title)
    )
}

/// `en dark square`: the variant without its profile.
fn short(gallery: &Gallery, variant: usize) -> String {
    let v = &gallery.variants[variant];
    format!(
        "{} {} {}",
        v.language,
        if v.night { "dark" } else { "light" },
        v.theme.name
    )
}

fn crumbs(from: &str, plugin: &str) -> String {
    format!(
        "<p><a href=\"{}\">Gallery</a> / <a href=\"{}#{}\">{}</a></p>",
        link(from, "index.html"),
        link(from, "index.html"),
        escape(plugin),
        escape(plugin)
    )
}

fn index_html(gallery: &Gallery, sheets: &Sheets<'_>) -> String {
    let mut body = String::from(STYLE);
    let _ = write!(
        body,
        "<h1>Gallery</h1><p>{} variants on {}. Each screen and widget has a page with all of them; \
         widgets in sizes {}{}.</p><p class=notes>A contact sheet (PNG) per plugin and profile: a column \
         per variant (language, light before dark, then the looks), a row per screen, then a row per \
         widget with its sizes laid out like the home grid (a line per height, widths growing to the \
         right). Pictures at device size unless the sheet says it is shrunk.</p>",
        gallery.variants.len(),
        escape(&gallery.profiles().join(", ")),
        gallery
            .sizes
            .iter()
            .map(|(w, h)| format!("{w}x{h}"))
            .collect::<Vec<_>>()
            .join(", "),
        if gallery.quick {
            ""
        } else {
            " (all of them in the first variant of each profile and language, a few in the others)"
        }
    );
    let names: Vec<String> = gallery
        .plugins
        .keys()
        .map(|p| format!("<a href=\"#{0}\">{0}</a>", escape(p)))
        .collect();
    let _ = write!(body, "<p>Plugins: {}</p>", names.join(" · "));
    for (plugin, cases) in &gallery.plugins {
        let _ = write!(
            body,
            "<h2 id=\"{0}\">{0}</h2><p>Contact sheets:",
            escape(plugin)
        );
        for profile in gallery.profiles() {
            if let Some((parts, factor)) = sheets.get(&(plugin.clone(), profile)) {
                let links: Vec<String> = parts
                    .iter()
                    .enumerate()
                    .map(|(n, path)| {
                        let name = if n == 0 {
                            escape(profile)
                        } else {
                            (n + 1).to_string()
                        };
                        format!("<a href=\"{}\">{name}</a>", escape(path))
                    })
                    .collect();
                let shrunk = if *factor > 1 {
                    format!(", shrunk 1:{factor}")
                } else {
                    String::new()
                };
                let more = if parts.len() > 1 {
                    format!(" (in {} parts{shrunk})", parts.len())
                } else if *factor > 1 {
                    format!(" (shrunk 1:{factor})")
                } else {
                    String::new()
                };
                let _ = write!(body, " {}{more}", links.join(" "));
            }
        }
        body.push_str("</p>");
        if !cases.screens.is_empty() {
            body.push_str("<h3>Screens</h3><div class=sheet>");
            for (id, case) in &cases.screens {
                let page = screen_page(plugin, id);
                let shown = case.shots.values().next().map_or_else(
                    || String::from("<span class=none>not shown: needs parameters</span>"),
                    |pic| thumb(pic, id),
                );
                let _ = write!(
                    body,
                    "<figure class=thumb><a href=\"{}\">{shown}</a><figcaption>{}</figcaption></figure>",
                    escape(&page),
                    escape(id)
                );
            }
            body.push_str("</div>");
        }
        if !cases.widgets.is_empty() {
            body.push_str("<h3>Widgets</h3><div class=sheet>");
            for (id, case) in &cases.widgets {
                let page = widget_page(plugin, id);
                let shown = case.context.values().next().map_or_else(
                    || String::from("<span class=none>not placed</span>"),
                    |pic| thumb(pic, id),
                );
                let _ = write!(
                    body,
                    "<figure class=thumb><a href=\"{}\">{shown}</a><figcaption>{}<br><code>{}</code></figcaption></figure>",
                    escape(&page),
                    escape(&case.title),
                    escape(id)
                );
            }
            body.push_str("</div>");
        }
    }
    report::page("PaperPaw gallery", &body, "")
}

fn screen_html(gallery: &Gallery, plugin: &str, id: &str, from: &str) -> String {
    let case = &gallery.plugins[plugin].screens[id];
    let mut body = String::from(STYLE);
    body.push_str(&crumbs(from, plugin));
    let _ = write!(body, "<h1>{}</h1>", escape(id));
    match case.params {
        None => body.push_str(
            "<p class=notes>Does not open without parameters (nor with a book, a folder, a word or a text): not shown.</p>",
        ),
        Some(0) => {}
        Some(index) => {
            let given: Vec<String> = super::PARAMS[index]
                .iter()
                .map(|(k, v)| format!("{k} = {v}"))
                .collect();
            let _ = write!(
                body,
                "<p class=notes>Opened with {}.</p>",
                escape(&given.join(", "))
            );
        }
    }
    body.push_str(&scale_buttons("0.5"));
    for profile in gallery.profiles() {
        let _ = write!(body, "<h2>{}</h2><div class=sheet>", escape(profile));
        for v in gallery.of_profile(profile) {
            let label = short(gallery, v);
            let shown = case.shots.get(&v).map_or_else(
                || String::from("<span class=none>not opened</span>"),
                |pic| img(from, pic, &label),
            );
            let _ = write!(
                body,
                "<figure>{shown}<figcaption>{}</figcaption></figure>",
                escape(&label)
            );
        }
        body.push_str("</div>");
    }
    report::page(id, &body, SCALE_SCRIPT)
}

fn widget_html(gallery: &Gallery, plugin: &str, id: &str, from: &str) -> String {
    let case = &gallery.plugins[plugin].widgets[id];
    let mut body = String::from(STYLE);
    body.push_str(&crumbs(from, plugin));
    let _ = write!(
        body,
        "<h1>{} <code>{}</code></h1><p class=notes>Every size, a line per height. Sizes it draws \
         nothing in are outlined; sizes the grid did not place are left as dashed boxes.</p>",
        escape(&case.title),
        escape(id)
    );
    body.push_str(&scale_buttons("1"));
    for profile in gallery.profiles() {
        let _ = write!(body, "<h2>{}</h2><div class=sheet>", escape(profile));
        for v in gallery.of_profile(profile) {
            let label = short(gallery, v);
            let whole = case.context.get(&v).map_or(String::new(), |pic| {
                format!(
                    " <a href=\"{}\">whole screen</a>",
                    escape(&link(from, &pic.path))
                )
            });
            let _ = write!(
                body,
                "<figure><figcaption>{}{whole}</figcaption><div class=lines>",
                escape(&label)
            );
            for line in lines(gallery) {
                body.push_str("<div class=line>");
                for size in line {
                    let name = format!("{}x{}", size.0, size.1);
                    let shot = case.shots.iter().find(|s| s.variant == v && s.size == size);
                    let Some(shot) = shot else { continue };
                    match &shot.pic {
                        Some(pic) => {
                            let class = if shot.empty { "slot empty" } else { "slot" };
                            let _ = write!(
                                body,
                                "<div class=\"{class}\">{name}{}</div>",
                                img(from, pic, &format!("{name} {label}"))
                            );
                        }
                        None => {
                            let (w, h) = gallery.cells.get(&(v, size)).copied().unwrap_or((40, 40));
                            let _ = write!(
                                body,
                                "<div class=slot>{name}<div class=ph data-w={w} data-h={h}>not placed</div></div>"
                            );
                        }
                    }
                }
                body.push_str("</div>");
            }
            body.push_str("</div></figure>");
        }
        body.push_str("</div>");
    }
    report::page(id, &body, SCALE_SCRIPT)
}

/// The sizes looked at, a line per height, widths growing.
fn lines(gallery: &Gallery) -> Vec<Vec<(u8, u8)>> {
    let mut lines: Vec<Vec<(u8, u8)>> = Vec::new();
    for &size in &gallery.sizes {
        match lines.last_mut() {
            Some(line) if line[0].1 == size.1 => line.push(size),
            _ => lines.push(vec![size]),
        }
    }
    lines
}

/// The size of a widget's sizes laid out for variant `v`, shrunk by
/// `k`, and where each size goes in it.
fn widget_block(gallery: &Gallery, v: usize, k: u32) -> Block {
    let (mut width, mut height) = (0u32, 0u32);
    let mut places = Vec::new();
    for line in lines(gallery) {
        let (mut x, mut tallest) = (0u32, 0u32);
        for size in line {
            let Some(&(w, h)) = gallery.cells.get(&(v, size)) else {
                continue;
            };
            if x > 0 {
                x += INNER;
            }
            places.push((size, x, height));
            x += w / k + 2;
            tallest = tallest.max(h / k + 2);
        }
        if tallest > 0 {
            width = width.max(x);
            height += tallest + INNER;
        }
    }
    (width, height.saturating_sub(INNER), places)
}

/// A row of a contact sheet.
enum Row<'a> {
    Screen(&'a super::ScreenCase),
    Widget(&'a super::WidgetCase),
}

/// Writes the contact sheets of a plugin's cases on `profile`: the parts
/// written (their paths in the gallery) and the factor the pictures were
/// shrunk by; nothing if nothing was shown there.
fn sheets(
    gallery: &Gallery,
    plugin: &str,
    cases: &PluginCases,
    profile: &str,
) -> Result<(Vec<String>, u32), String> {
    let columns = gallery.of_profile(profile);
    let mut rows: Vec<Row<'_>> = cases
        .screens
        .values()
        .filter(|c| columns.iter().any(|v| c.shots.contains_key(v)))
        .map(Row::Screen)
        .collect();
    rows.extend(
        cases
            .widgets
            .values()
            .filter(|c| {
                c.shots
                    .iter()
                    .any(|s| columns.contains(&s.variant) && s.pic.is_some())
            })
            .map(Row::Widget),
    );
    if rows.is_empty() {
        return Ok((Vec::new(), 1));
    }
    let screen_size = rows
        .iter()
        .find_map(|row| match row {
            Row::Screen(c) => columns.iter().find_map(|v| c.shots.get(v)),
            Row::Widget(_) => None,
        })
        .map_or((0, 0), |p| (p.width, p.height));
    // Sizes at a factor: a screen's place, each column's widget block.
    let measure = |k: u32| {
        let screen = (screen_size.0 / k + 2, screen_size.1 / k + 2);
        let blocks: Vec<_> = columns
            .iter()
            .map(|&v| widget_block(gallery, v, k))
            .collect();
        let n = columns.len() as u32;
        let screens_w = n * screen.0 + (n + 1) * GAP;
        let widgets_w = blocks.iter().map(|b| b.0 + GAP).sum::<u32>() + GAP;
        let block_h = blocks.iter().map(|b| b.1).max().unwrap_or(0);
        Layout {
            screen,
            blocks,
            screens_w,
            widgets_w,
            block_h,
        }
    };
    let row_size = |row: &Row<'_>, m: &Layout| -> (u32, u32) {
        match row {
            Row::Screen(_) => (m.screens_w, m.screen.1 + GAP),
            Row::Widget(_) => (m.widgets_w, m.block_h + GAP),
        }
    };
    // The smallest factor at which every row fits a sheet on its own.
    let mut k = 1;
    let m = loop {
        let m = measure(k);
        let fits = rows.iter().all(|row| {
            let (w, h) = row_size(row, &m);
            u64::from(w) * u64::from(h + SECTION + GAP) <= BUDGET
        });
        if fits || k >= 16 {
            break m;
        }
        k += 1;
    };
    // Rows into parts of at most the budget each, in order.
    let mut parts: Vec<Vec<&Row<'_>>> = vec![Vec::new()];
    let (mut width, mut height) = (0u32, GAP);
    for row in &rows {
        let (w, h) = row_size(row, &m);
        let extra = if matches!(row, Row::Widget(_))
            && parts
                .last()
                .and_then(|p| p.last())
                .is_some_and(|r| matches!(r, Row::Screen(_)))
        {
            SECTION - GAP
        } else {
            0
        };
        let (new_w, new_h) = (width.max(w), height + extra + h);
        let last = parts.last_mut().map_or(0, |p| p.len());
        if last > 0 && u64::from(new_w) * u64::from(new_h) > BUDGET {
            parts.push(vec![row]);
            (width, height) = (w, GAP + h);
        } else {
            if let Some(part) = parts.last_mut() {
                part.push(row);
            }
            (width, height) = (new_w, new_h);
        }
    }
    let mut written = Vec::new();
    for (n, part) in parts.iter().enumerate() {
        let path = sheet_path(plugin, profile, n);
        let sheet = draw_sheet(gallery, &columns, part, &m, k)?;
        super::save_png(&sheet, &gallery.dir.join(&path))?;
        written.push(path);
    }
    Ok((written, k))
}

/// Where things go on a contact sheet at a factor.
struct Layout {
    /// A screen's place (with its frame).
    screen: (u32, u32),
    /// Per column, the widget block: width, height, and where each size
    /// goes in it.
    blocks: Vec<Block>,
    screens_w: u32,
    widgets_w: u32,
    /// The tallest block.
    block_h: u32,
}

/// A widget's sizes laid out: width, height, and each size's place.
type Block = (u32, u32, Vec<((u8, u8), u32, u32)>);

/// Draws one part of a contact sheet: its rows, the pictures shrunk by
/// `k`, placed as `m` says.
fn draw_sheet(
    gallery: &Gallery,
    columns: &[usize],
    rows: &[&Row<'_>],
    m: &Layout,
    k: u32,
) -> Result<Image, String> {
    let Layout {
        screen,
        blocks,
        screens_w,
        widgets_w,
        block_h,
    } = m;
    let mut width = 0;
    let mut height = GAP;
    let mut previous_screen = false;
    for row in rows {
        match row {
            Row::Screen(_) => {
                width = width.max(*screens_w);
                height += screen.1 + GAP;
                previous_screen = true;
            }
            Row::Widget(_) => {
                width = width.max(*widgets_w);
                if previous_screen {
                    height += SECTION - GAP;
                    previous_screen = false;
                }
                height += block_h + GAP;
            }
        }
    }
    let mut sheet = Image::filled(width.max(1), height, BACKGROUND);
    // Pictures loaded once a part: a picture often stands for several
    // variants that look the same.
    let mut loaded: HashMap<String, Image> = HashMap::new();
    let mut paste = |sheet: &mut Image, pic: &Pic, x: u32, y: u32| -> Result<(), String> {
        if !loaded.contains_key(&pic.path) {
            let mut image = Image::load_png(&gallery.dir.join(&pic.path))?;
            if k > 1 {
                image = image.shrink(k);
            }
            loaded.insert(pic.path.clone(), image);
        }
        if let Some(image) = loaded.get(&pic.path) {
            framed(sheet, image, x, y);
        }
        Ok(())
    };
    let mut y = GAP;
    let mut previous_screen = false;
    for row in rows {
        match row {
            Row::Screen(case) => {
                for (column, v) in columns.iter().enumerate() {
                    if let Some(pic) = case.shots.get(v) {
                        let x = GAP + column as u32 * (screen.0 + GAP);
                        paste(&mut sheet, pic, x, y)?;
                    }
                }
                y += screen.1 + GAP;
                previous_screen = true;
            }
            Row::Widget(case) => {
                if previous_screen {
                    y += SECTION - GAP;
                    previous_screen = false;
                }
                let mut x = GAP;
                for (column, &v) in columns.iter().enumerate() {
                    let (block_w, _, places) = &blocks[column];
                    for &(size, dx, dy) in places {
                        let shot = case.shots.iter().find(|s| s.variant == v && s.size == size);
                        if let Some(pic) = shot.and_then(|s| s.pic.as_ref()) {
                            paste(&mut sheet, pic, x + dx, y + dy)?;
                        }
                    }
                    x += block_w + GAP;
                }
                y += block_h + GAP;
            }
        }
    }
    Ok(sheet)
}

/// Pastes `picture` with a 1 pixel frame around it, the frame's top left
/// corner at `x`, `y`.
fn framed(sheet: &mut Image, picture: &Image, x: u32, y: u32) {
    let (w, h) = (picture.width + 2, picture.height + 2);
    for dx in 0..w {
        sheet.set_pixel(x + dx, y, FRAME);
        sheet.set_pixel(x + dx, y + h - 1, FRAME);
    }
    for dy in 0..h {
        sheet.set_pixel(x, y + dy, FRAME);
        sheet.set_pixel(x + w - 1, y + dy, FRAME);
    }
    // Row by row: a sheet takes many large pictures.
    let row = (picture.width * 3) as usize;
    for line in 0..picture.height {
        let to = (((y + 1 + line) * sheet.width + x + 1) * 3) as usize;
        let from = (line * picture.width * 3) as usize;
        if let (Some(target), Some(source)) = (
            sheet.rgb.get_mut(to..to + row),
            picture.rgb.get(from..from + row),
        ) {
            target.copy_from_slice(source);
        }
    }
}
