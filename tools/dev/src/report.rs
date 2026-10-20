//! What the checks found, written for people (the console,
//! `report.html`) and for machines (`report.json`).
//!
//! A finding names its check (`visual.text-cut`), how bad it is, the
//! plugin and place it is about (a screen, a widget in a size, a file),
//! the variant it was seen in (device, language, light or dark, look)
//! and, for what is drawn, a picture with the area marked. The same
//! finding seen in several variants is listed once, with all of them.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use paperpaw_platform::Rect;

/// How bad a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Worth knowing, nothing wrong (e.g. a measurement).
    Info,
    /// Probably wrong: fails the run with `--strict`.
    Warning,
    /// Wrong: fails the run.
    Error,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Warning => "warning",
            Level::Error => "error",
        }
    }
}

/// One thing a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The check, `area.name` (e.g. `visual.text-cut`, `lint.dash`).
    pub check: &'static str,
    pub level: Level,
    /// The plugin it is about, by id, if known.
    pub plugin: Option<String>,
    /// Where: a screen's id, a widget and its size, a file, a key.
    pub place: String,
    /// What is wrong, in a sentence.
    pub message: String,
    /// The variant it was seen in (see [`crate::variant::Variant::name`]).
    pub variant: Option<String>,
    /// A picture of it, relative to the output folder.
    pub picture: Option<PathBuf>,
    /// Where in the picture.
    pub area: Option<Rect>,
}

impl Finding {
    pub fn new(
        check: &'static str,
        level: Level,
        place: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Finding {
            check,
            level,
            plugin: None,
            place: place.into(),
            message: message.into(),
            variant: None,
            picture: None,
            area: None,
        }
    }

    pub fn error(
        check: &'static str,
        place: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(check, Level::Error, place, message)
    }

    pub fn warning(
        check: &'static str,
        place: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(check, Level::Warning, place, message)
    }

    pub fn info(check: &'static str, place: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(check, Level::Info, place, message)
    }

    pub fn plugin(mut self, plugin: impl Into<String>) -> Self {
        self.plugin = Some(plugin.into());
        self
    }

    pub fn variant(mut self, variant: impl Into<String>) -> Self {
        self.variant = Some(variant.into());
        self
    }

    pub fn picture(mut self, picture: impl Into<PathBuf>) -> Self {
        self.picture = Some(picture.into());
        self
    }

    pub fn area(mut self, area: Rect) -> Self {
        self.area = Some(area);
        self
    }
}

/// A page a command made besides the findings (a contact sheet, a
/// gallery, a table of measurements), linked from `report.html`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub title: String,
    /// Relative to the output folder.
    pub path: PathBuf,
}

/// Everything found in a run.
#[derive(Debug, Default)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub pages: Vec<Page>,
    /// Which commands ran, in order.
    pub ran: Vec<String>,
}

/// Findings that are the same but for the variant, with every variant.
#[derive(Debug)]
pub struct Group<'a> {
    pub first: &'a Finding,
    pub variants: Vec<&'a str>,
}

impl Report {
    pub fn add(&mut self, finding: Finding) {
        self.findings.push(finding);
    }

    pub fn page(&mut self, title: impl Into<String>, path: impl Into<PathBuf>) {
        self.pages.push(Page {
            title: title.into(),
            path: path.into(),
        });
    }

    pub fn count(&self, level: Level) -> usize {
        self.groups()
            .iter()
            .filter(|g| g.first.level == level)
            .count()
    }

    /// Whether the run fails: errors, or warnings too if `strict`.
    pub fn fails(&self, strict: bool) -> bool {
        self.count(Level::Error) > 0 || (strict && self.count(Level::Warning) > 0)
    }

    /// The findings grouped (same check, plugin, place and message), the
    /// worst first, then by check and place.
    pub fn groups(&self) -> Vec<Group<'_>> {
        let mut groups: Vec<Group<'_>> = Vec::new();
        for finding in &self.findings {
            let same = groups.iter_mut().find(|g| {
                g.first.check == finding.check
                    && g.first.level == finding.level
                    && g.first.plugin == finding.plugin
                    && g.first.place == finding.place
                    && g.first.message == finding.message
            });
            match same {
                Some(group) => {
                    if let Some(variant) = &finding.variant {
                        group.variants.push(variant);
                    }
                }
                None => groups.push(Group {
                    first: finding,
                    variants: finding.variant.as_deref().into_iter().collect(),
                }),
            }
        }
        groups.sort_by(|a, b| {
            b.first
                .level
                .cmp(&a.first.level)
                .then(a.first.check.cmp(b.first.check))
                .then(a.first.plugin.cmp(&b.first.plugin))
                .then(a.first.place.cmp(&b.first.place))
        });
        groups
    }

    /// The findings for the console, at least `level`.
    pub fn text(&self, level: Level) -> String {
        let mut out = String::new();
        for group in self.groups() {
            let f = group.first;
            if f.level < level {
                continue;
            }
            let plugin = f
                .plugin
                .as_deref()
                .map(|p| format!("{p}: "))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "{}[{}] {plugin}{}: {}",
                f.level.name(),
                f.check,
                f.place,
                f.message
            );
            match group.variants.len() {
                0 => {}
                1 => {
                    let _ = writeln!(out, "    in {}", group.variants[0]);
                }
                n => {
                    let _ = writeln!(out, "    in {} and {} more", group.variants[0], n - 1);
                }
            }
            if let Some(picture) = &f.picture {
                let _ = writeln!(out, "    see {}", picture.display());
            }
        }
        let _ = writeln!(
            out,
            "{} errors, {} warnings, {} notes ({})",
            self.count(Level::Error),
            self.count(Level::Warning),
            self.count(Level::Info),
            self.ran.join(", ")
        );
        out
    }

    /// The findings as JSON, one object each (variants listed).
    pub fn json(&self) -> String {
        let mut out = String::from("{\n  \"findings\": [");
        for (i, group) in self.groups().iter().enumerate() {
            let f = group.first;
            out.push_str(if i == 0 { "\n    {" } else { ",\n    {" });
            let _ = write!(
                out,
                "\"check\": {}, \"level\": {}, \"plugin\": {}, \"place\": {}, \"message\": {}",
                json_string(f.check),
                json_string(f.level.name()),
                f.plugin.as_deref().map_or("null".into(), json_string),
                json_string(&f.place),
                json_string(&f.message),
            );
            let variants: Vec<String> = group.variants.iter().map(|v| json_string(v)).collect();
            let _ = write!(out, ", \"variants\": [{}]", variants.join(", "));
            if let Some(picture) = &f.picture {
                let _ = write!(out, ", \"picture\": {}", json_string(&slashed(picture)));
            }
            if let Some(a) = f.area {
                let _ = write!(
                    out,
                    ", \"area\": [{}, {}, {}, {}]",
                    a.x, a.y, a.width, a.height
                );
            }
            out.push('}');
        }
        out.push_str("\n  ],\n  \"pages\": [");
        for (i, page) in self.pages.iter().enumerate() {
            out.push_str(if i == 0 { "\n    " } else { ",\n    " });
            let _ = write!(
                out,
                "{{\"title\": {}, \"path\": {}}}",
                json_string(&page.title),
                json_string(&slashed(&page.path))
            );
        }
        let _ = write!(
            out,
            "\n  ],\n  \"errors\": {},\n  \"warnings\": {}\n}}\n",
            self.count(Level::Error),
            self.count(Level::Warning)
        );
        out
    }

    /// `report.html`: the pages made, then the findings with their
    /// pictures, the area marked.
    pub fn html(&self) -> String {
        let mut body = String::new();
        let _ = write!(
            body,
            "<h1>PaperPaw plugin check</h1><p class=sum>{} errors, {} warnings, {} notes. Ran: {}.</p>",
            self.count(Level::Error),
            self.count(Level::Warning),
            self.count(Level::Info),
            escape(&self.ran.join(", "))
        );
        if !self.pages.is_empty() {
            body.push_str("<h2>Pages</h2><ul>");
            for page in &self.pages {
                let _ = write!(
                    body,
                    "<li><a href=\"{}\">{}</a></li>",
                    escape(&slashed(&page.path)),
                    escape(&page.title)
                );
            }
            body.push_str("</ul>");
        }
        body.push_str("<h2>Findings</h2>");
        let mut check = "";
        for group in self.groups() {
            let f = group.first;
            if f.check != check {
                check = f.check;
                let _ = write!(body, "<h3>{}</h3>", escape(check));
            }
            let plugin = f
                .plugin
                .as_deref()
                .map(|p| format!("{}: ", escape(p)))
                .unwrap_or_default();
            let _ = write!(
                body,
                "<div class=\"f {}\"><p><b>{}</b> {plugin}<code>{}</code>: {}</p>",
                f.level.name(),
                f.level.name(),
                escape(&f.place),
                escape(&f.message)
            );
            if !group.variants.is_empty() {
                let _ = write!(
                    body,
                    "<p class=v>{}</p>",
                    escape(&group.variants.join("; "))
                );
            }
            if let Some(picture) = &f.picture {
                let mark = f.area.map_or(String::new(), |a| {
                    format!(
                        "<svg class=mark viewBox=\"0 0 1 1\" preserveAspectRatio=\"none\" data-area=\"{} {} {} {}\"></svg>",
                        a.x, a.y, a.width, a.height
                    )
                });
                let _ = write!(
                    body,
                    "<a class=pic href=\"{0}\"><img src=\"{0}\" loading=lazy>{mark}</a>",
                    escape(&slashed(picture))
                );
            }
            body.push_str("</div>");
        }
        page("PaperPaw plugin check", &body, MARK_SCRIPT)
    }

    /// Writes `report.html` and `report.json` into `out`.
    pub fn write(&self, out: &Path) -> Result<(), String> {
        std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
        for (name, text) in [("report.html", self.html()), ("report.json", self.json())] {
            let path = out.join(name);
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(())
    }
}

/// Draws the marked areas over their pictures, once they are loaded.
const MARK_SCRIPT: &str = "for (const svg of document.querySelectorAll('svg.mark')) {\
  const img = svg.previousElementSibling;\
  const place = () => { const [x, y, w, h] = svg.dataset.area.split(' ').map(Number);\
    const sx = img.clientWidth / img.naturalWidth, sy = img.clientHeight / img.naturalHeight;\
    Object.assign(svg.style, {left: x * sx + 'px', top: y * sy + 'px', width: Math.max(w * sx, 4) + 'px', height: Math.max(h * sy, 4) + 'px'}); };\
  if (img.complete) place(); else img.addEventListener('load', place); }";

/// A whole HTML page with the report's style: for every page the tool
/// writes.
pub fn page(title: &str, body: &str, script: &str) -> String {
    format!(
        "<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content=\"width=device-width\">\
<title>{}</title><style>{STYLE}</style><body>{body}<script>{script}</script></body></html>\n",
        escape(title)
    )
}

const STYLE: &str = "body{font:15px/1.4 system-ui,sans-serif;margin:16px;background:#fafafa;color:#222}\
h1{font-size:22px}h2{font-size:18px;margin-top:28px}h3{font-size:16px;margin:20px 0 6px}\
code{background:#eee;padding:0 3px}.sum{font-size:16px}.v{color:#666;font-size:13px;margin:2px 0}\
.f{border-left:4px solid #999;padding:4px 10px;margin:6px 0;background:#fff}\
.f.error{border-color:#c00}.f.warning{border-color:#d80}.f.info{border-color:#58a}.f p{margin:2px 0}\
.pic{position:relative;display:inline-block;margin:4px 0}.pic img{max-width:320px;max-height:420px;border:1px solid #ccc;image-rendering:pixelated}\
.mark{position:absolute;border:2px solid #e00;box-sizing:border-box}\
.sheet{display:flex;flex-wrap:wrap;gap:12px;align-items:flex-start}\
.sheet figure{margin:0;background:#fff;border:1px solid #ddd;padding:6px}\
.sheet img{display:block;image-rendering:pixelated;border:1px solid #ccc}\
.sheet figcaption{font-size:12px;color:#444;max-width:260px;overflow-wrap:anywhere}\
table{border-collapse:collapse}td,th{border:1px solid #ddd;padding:3px 8px;text-align:left}\
@media(prefers-color-scheme:dark){body{background:#181818;color:#ddd}.f,.sheet figure{background:#222;border-color:#333}code{background:#333}.sheet figcaption{color:#aaa}td,th{border-color:#444}}";

/// `text` safe inside HTML.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// `text` as a JSON string.
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A path with forward slashes, for links.
pub fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_finding_in_two_variants_is_one() {
        let mut report = Report::default();
        for variant in ["xteink-x4 en light", "boox-go-7 de dark"] {
            report.add(
                Finding::error("visual.text-cut", "hello.screen", "\"Hello\" is cut")
                    .plugin("hello")
                    .variant(variant),
            );
        }
        report.add(Finding::warning("lint.dash", "i18n/hello.en.ini", "a dash"));
        let groups = report.groups();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].variants.len(), 2);
        assert!(report.fails(false));
        let text = report.text(Level::Info);
        assert!(text.contains("in xteink-x4 en light and 1 more"), "{text}");
        assert!(text.contains("1 errors, 1 warnings"), "{text}");
    }

    #[test]
    fn json_and_html_are_escaped() {
        let mut report = Report::default();
        report.add(Finding::warning("lint.dash", "a \"b\" <c>", "x\ny"));
        let json = report.json();
        assert!(json.contains(r#""place": "a \"b\" <c>""#), "{json}");
        assert!(json.contains(r#""message": "x\ny""#), "{json}");
        assert!(report.html().contains("a &quot;b&quot; &lt;c&gt;"));
    }
}
