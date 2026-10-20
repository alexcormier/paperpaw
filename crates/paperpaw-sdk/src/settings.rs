use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::collections::VecMap;
use crate::id::Id;
use crate::plugin::RegisterError;
use crate::setting::{Setting, Text};

/// A setting's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Text(Cow<'static, str>),
}

/// The type and valid range of a setting. Settings UIs are generated from it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingKind {
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    /// One of a fixed set of values.
    Choice(Choices),
    Text,
    /// A UI language tag; settings UIs offer the installed translations.
    Language,
    /// Ids separated by commas, e.g. the status items of a status line;
    /// shown by their names, chosen on the setting's editor screen.
    List,
}

/// The values of a [`SettingKind::Choice`], each the translation key of
/// its name: a list in the program ([`Choices::Fixed`], costing no
/// memory) or one made at run time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choices {
    Fixed(&'static [&'static str]),
    Owned(Vec<Cow<'static, str>>),
}

impl Choices {
    /// The values in their order.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        let (fixed, owned): (&[&str], &[Cow<'static, str>]) = match self {
            Choices::Fixed(values) => (values, &[]),
            Choices::Owned(values) => (&[], values),
        };
        fixed
            .iter()
            .copied()
            .chain(owned.iter().map(|v| v.as_ref()))
    }

    pub fn len(&self) -> usize {
        match self {
            Choices::Fixed(values) => values.len(),
            Choices::Owned(values) => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Value `index`.
    pub fn get(&self, index: usize) -> Option<&str> {
        match self {
            Choices::Fixed(values) => values.get(index).copied(),
            Choices::Owned(values) => values.get(index).map(|v| v.as_ref()),
        }
    }

    /// Where `value` is among them.
    pub fn position(&self, value: &str) -> Option<usize> {
        self.iter().position(|v| v == value)
    }

    pub fn contains(&self, value: &str) -> bool {
        self.position(value).is_some()
    }
}

impl From<Vec<Cow<'static, str>>> for Choices {
    fn from(values: Vec<Cow<'static, str>>) -> Self {
        Choices::Owned(values)
    }
}

impl FromIterator<Cow<'static, str>> for Choices {
    fn from_iter<I: IntoIterator<Item = Cow<'static, str>>>(values: I) -> Self {
        Choices::Owned(values.into_iter().collect())
    }
}

impl From<&'static [&'static str]> for Choices {
    fn from(values: &'static [&'static str]) -> Self {
        Choices::Fixed(values)
    }
}

impl SettingKind {
    /// Parses a value as written in a configuration file.
    pub fn parse(&self, text: &str) -> Option<SettingValue> {
        let value = match self {
            SettingKind::Bool => SettingValue::Bool(match text {
                "true" | "yes" | "on" | "1" => true,
                "false" | "no" | "off" | "0" => false,
                _ => return None,
            }),
            SettingKind::Int { .. } => SettingValue::Int(text.parse().ok()?),
            SettingKind::Choice(_)
            | SettingKind::Text
            | SettingKind::Language
            | SettingKind::List => SettingValue::Text(Cow::Owned(String::from(text))),
        };
        self.accepts(&value).then_some(value)
    }

    pub fn accepts(&self, value: &SettingValue) -> bool {
        match (self, value) {
            (SettingKind::Bool, SettingValue::Bool(_)) => true,
            (SettingKind::Int { min, max }, SettingValue::Int(v)) => (min..=max).contains(&v),
            (SettingKind::Choice(options), SettingValue::Text(v)) => options.contains(v),
            (SettingKind::Text | SettingKind::List, SettingValue::Text(_)) => true,
            (SettingKind::Language, SettingValue::Text(v)) => {
                !v.is_empty() && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }
            _ => false,
        }
    }
}

impl SettingValue {
    /// The value as written in configuration files.
    pub fn to_config_text(&self) -> String {
        match self {
            SettingValue::Bool(true) => String::from("on"),
            SettingValue::Bool(false) => String::from("off"),
            SettingValue::Int(v) => alloc::format!("{v}"),
            SettingValue::Text(v) => String::from(v.as_ref()),
        }
    }
}

/// A setting contribution, made with [`SettingSpec::new`] or the typed
/// constructors ([`SettingSpec::switch`] and the others) and the builder
/// methods.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SettingSpec {
    /// `<plugin>.<name>`; in configuration files written as `[<plugin>] <name> = …`.
    pub id: Id,
    /// Translation key of the setting's display name.
    pub title: Cow<'static, str>,
    pub kind: SettingKind,
    pub default: SettingValue,
    /// The group it is shown in, e.g.
    /// [`builtin::groups::READING`](crate::builtin::groups::READING).
    pub group: Id,
    pub level: Level,
    /// Translation key of a short help text.
    pub description: Option<Cow<'static, str>>,
    /// May differ per scope, e.g. per book (see [`Settings::scope`]).
    pub scoped: bool,
    /// What few settings have ([`SettingSpec::extra`]), kept apart: the
    /// others take less memory.
    extra: Option<Extra>,
}

/// Where a spec's [`SettingExtra`] is: in the program, or on the heap.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Extra {
    Fixed(&'static SettingExtra),
    Owned(Box<SettingExtra>),
}

/// What few settings have (see [`SettingSpec::extra`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SettingExtra {
    /// Translation key that formats a number with its unit, with plural
    /// forms, e.g. `core.unit.pt.other = {count} pt`.
    pub unit: Option<Cow<'static, str>>,
    /// A screen that edits the value, opened with the parameter `id` (the
    /// setting's id), e.g. a folder chooser for a path.
    pub editor: Option<Id>,
    /// How much one step changes a number (e.g. 10 for percentages).
    pub step: i64,
    /// A number better set by eye than by its value, such as the line
    /// spacing or a light's brightness: on touch screens settings pages
    /// and panels show it as a slider ([`ui::slider`](crate::ui::slider)),
    /// in its steps; with buttons alone it keeps `−` and `+`.
    pub slider: bool,
    /// Translation key of the part of its group it belongs to (e.g.
    /// [`setting_sections::FONT`](crate::builtin::setting_sections::FONT)),
    /// shown as a heading above it in large groups.
    pub section: Option<Cow<'static, str>>,
    /// A secret such as an API key: shown as dots, and entered afresh
    /// rather than edited.
    pub secret: bool,
    /// Another setting of the same kind whose value this one has until it
    /// is set itself, e.g. the margins at the top and bottom those at the
    /// sides (one step only: what that one follows is not followed).
    pub follows: Option<&'static str>,
}

/// The extra of settings without one.
static NO_EXTRA: SettingExtra = SettingExtra::new();

impl SettingExtra {
    /// None of them: no unit, editor or section, a step of 1. Made in the
    /// program for fixed settings with the builder methods below and given
    /// to [`SettingSpec::with_extra`]:
    ///
    /// ```
    /// use paperpaw_sdk::{Id, SettingExtra, SettingSpec};
    /// const GROUP: Id = Id::new_static("my.page");
    /// static PERCENT: SettingExtra = SettingExtra::new().unit("my.unit.percent").step(10);
    /// static SETTINGS: [SettingSpec; 1] = [
    ///     SettingSpec::fixed_number("my.zoom", "my.zoom", 50, 200, 100, GROUP)
    ///         .with_extra(&PERCENT),
    /// ];
    /// ```
    pub const fn new() -> Self {
        Self {
            unit: None,
            editor: None,
            step: 1,
            slider: false,
            section: None,
            secret: false,
            follows: None,
        }
    }

    /// With the translation key of its unit ([`SettingExtra::unit`]).
    pub const fn unit(mut self, key: &'static str) -> Self {
        core::mem::forget(self.unit.replace(Cow::Borrowed(key)));
        self
    }

    /// Edited on screen `screen` ([`SettingExtra::editor`]).
    pub const fn editor(mut self, screen: Id) -> Self {
        core::mem::forget(self.editor.replace(screen));
        self
    }

    /// One step (a `+` or `−`) changes a number by `step`.
    pub const fn step(mut self, step: i64) -> Self {
        self.step = if step < 1 { 1 } else { step };
        self
    }

    /// Set with a slider ([`SettingExtra::slider`]).
    pub const fn slider(mut self) -> Self {
        self.slider = true;
        self
    }

    /// In the part `key` of its group ([`SettingExtra::section`]).
    pub const fn section(mut self, key: &'static str) -> Self {
        core::mem::forget(self.section.replace(Cow::Borrowed(key)));
        self
    }

    /// A secret ([`SettingExtra::secret`]).
    pub const fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    /// With the value of setting `id` until set itself
    /// ([`SettingExtra::follows`]).
    pub const fn follows(mut self, id: &'static str) -> Self {
        self.follows = Some(id);
        self
    }
}

impl Default for SettingExtra {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingSpec {
    /// A basic setting in the [`OTHER`](crate::builtin::groups::OTHER)
    /// group; see the builder methods for the rest.
    pub fn new(
        id: impl Into<Id>,
        title: impl Into<Cow<'static, str>>,
        kind: SettingKind,
        default: SettingValue,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            kind,
            default,
            group: crate::builtin::groups::OTHER,
            level: Level::Basic,
            description: None,
            scoped: false,
            extra: None,
        }
    }

    /// A setting kept in the program rather than built at start: in a
    /// `static` list registered with
    /// [`Registrar::add_settings`](crate::Registrar::add_settings), it
    /// takes next to no memory (on small devices the list stays in flash).
    /// Plugins with many settings, such as widgets with their looks, use
    /// these. Its unit, editor and the like ([`SettingSpec::extra`]) are
    /// given with [`SettingSpec::with_extra`], also kept in the program.
    ///
    /// ```
    /// use paperpaw_sdk::{Id, SettingSpec};
    /// const GROUP: Id = Id::new_static("my.clock");
    /// static SETTINGS: [SettingSpec; 2] = [
    ///     SettingSpec::fixed_switch("my.clock.date", "my.clock.date", true, GROUP),
    ///     SettingSpec::fixed_choice(
    ///         "my.clock.face",
    ///         "my.clock.face",
    ///         &["my.plain", "my.ticks"],
    ///         "my.ticks",
    ///         GROUP,
    ///     ),
    /// ];
    /// ```
    pub const fn fixed(
        id: &'static str,
        title: &'static str,
        kind: SettingKind,
        default: SettingValue,
        group: Id,
    ) -> Self {
        Self {
            id: Id::new_static(id),
            title: Cow::Borrowed(title),
            kind,
            default,
            group,
            level: Level::Basic,
            description: None,
            scoped: false,
            extra: None,
        }
    }

    /// A fixed switch ([`SettingSpec::fixed`]).
    pub const fn fixed_switch(
        id: &'static str,
        title: &'static str,
        default: bool,
        group: Id,
    ) -> Self {
        Self::fixed(
            id,
            title,
            SettingKind::Bool,
            SettingValue::Bool(default),
            group,
        )
    }

    /// A fixed whole number from `min` to `max` ([`SettingSpec::fixed`]).
    pub const fn fixed_number(
        id: &'static str,
        title: &'static str,
        min: i64,
        max: i64,
        default: i64,
        group: Id,
    ) -> Self {
        Self::fixed(
            id,
            title,
            SettingKind::Int { min, max },
            SettingValue::Int(default),
            group,
        )
    }

    /// A fixed choice of `options` ([`SettingSpec::fixed`]).
    pub const fn fixed_choice(
        id: &'static str,
        title: &'static str,
        options: &'static [&'static str],
        default: &'static str,
        group: Id,
    ) -> Self {
        Self::fixed(
            id,
            title,
            SettingKind::Choice(Choices::Fixed(options)),
            SettingValue::Text(Cow::Borrowed(default)),
            group,
        )
    }

    /// A fixed text, typed in ([`SettingSpec::fixed`]).
    pub const fn fixed_text(
        id: &'static str,
        title: &'static str,
        default: &'static str,
        group: Id,
    ) -> Self {
        Self::fixed(
            id,
            title,
            SettingKind::Text,
            SettingValue::Text(Cow::Borrowed(default)),
            group,
        )
    }

    /// The same with a help text (for [`SettingSpec::fixed`] ones, which
    /// cannot use [`description`](Self::description)).
    pub const fn described(self, key: &'static str) -> Self {
        let mut spec = self;
        let old = spec.description.replace(Cow::Borrowed(key));
        core::mem::forget(old);
        spec
    }

    /// The same, shown only with advanced settings (for
    /// [`SettingSpec::fixed`] ones).
    pub const fn for_advanced(self) -> Self {
        let mut spec = self;
        spec.level = Level::Advanced;
        spec
    }

    /// The same with `extra` (its unit, section and the like), kept in the
    /// program too: for [`SettingSpec::fixed`] ones.
    pub const fn with_extra(mut self, extra: &'static SettingExtra) -> Self {
        core::mem::forget(self.extra.replace(Extra::Fixed(extra)));
        self
    }

    /// Its unit, editor, step, slider, section and whether it is a secret
    /// (the defaults for a setting that sets none of them).
    pub fn extra(&self) -> &SettingExtra {
        match &self.extra {
            Some(Extra::Fixed(extra)) => extra,
            Some(Extra::Owned(extra)) => extra,
            None => &NO_EXTRA,
        }
    }

    fn extra_mut(&mut self) -> &mut SettingExtra {
        if let Some(Extra::Fixed(extra)) = self.extra {
            self.extra = Some(Extra::Owned(Box::new(extra.clone())));
        }
        match self
            .extra
            .get_or_insert_with(|| Extra::Owned(Box::default()))
        {
            Extra::Owned(extra) => extra,
            Extra::Fixed(_) => unreachable!("made owned above"),
        }
    }

    /// A switch, on or off (see [`setting`](crate::setting)).
    pub fn switch(
        setting: &Setting<bool>,
        title: impl Into<Cow<'static, str>>,
        default: bool,
    ) -> Self {
        Self::new(
            setting,
            title,
            SettingKind::Bool,
            SettingValue::Bool(default),
        )
    }

    /// A whole number from `min` to `max`.
    pub fn number(
        setting: &Setting<i64>,
        title: impl Into<Cow<'static, str>>,
        min: i64,
        max: i64,
        default: i64,
    ) -> Self {
        Self::new(
            setting,
            title,
            SettingKind::Int { min, max },
            SettingValue::Int(default),
        )
    }

    /// One of `options`, each the translation key of its name.
    pub fn choice(
        setting: &Setting<Text>,
        title: impl Into<Cow<'static, str>>,
        options: Choices,
        default: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::new(
            setting,
            title,
            SettingKind::Choice(options),
            SettingValue::Text(default.into()),
        )
    }

    /// Text, typed in.
    pub fn text(
        setting: &Setting<Text>,
        title: impl Into<Cow<'static, str>>,
        default: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::new(
            setting,
            title,
            SettingKind::Text,
            SettingValue::Text(default.into()),
        )
    }

    /// A language: the tag of one of the installed translations.
    pub fn language(
        setting: &Setting<Text>,
        title: impl Into<Cow<'static, str>>,
        default: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::new(
            setting,
            title,
            SettingKind::Language,
            SettingValue::Text(default.into()),
        )
    }

    /// Ids separated by commas, chosen on the setting's editor screen
    /// ([`SettingKind::List`]).
    pub fn list(
        setting: &Setting<Text>,
        title: impl Into<Cow<'static, str>>,
        default: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::new(
            setting,
            title,
            SettingKind::List,
            SettingValue::Text(default.into()),
        )
    }

    /// Puts the setting into a part of its group (see
    /// [`SettingSpec::section`]).
    pub fn section(mut self, key: impl Into<Cow<'static, str>>) -> Self {
        self.extra_mut().section = Some(key.into());
        self
    }

    /// A number set with a slider (see [`SettingExtra::slider`]).
    pub fn slider(mut self) -> Self {
        self.extra_mut().slider = true;
        self
    }

    /// A secret, such as an API key (see [`SettingSpec::secret`]).
    pub fn secret(mut self) -> Self {
        self.extra_mut().secret = true;
        self
    }

    /// How much one step (a `+` or `−`) changes a number.
    pub fn step(mut self, step: i64) -> Self {
        self.extra_mut().step = step.max(1);
        self
    }

    /// May be set per scope, e.g. for one book only.
    pub const fn scoped(mut self) -> Self {
        self.scoped = true;
        self
    }

    pub fn group(mut self, group: Id) -> Self {
        self.group = group;
        self
    }

    /// Shown only with advanced settings.
    pub const fn advanced(mut self) -> Self {
        self.level = Level::Advanced;
        self
    }

    pub fn description(mut self, key: impl Into<Cow<'static, str>>) -> Self {
        self.description = Some(key.into());
        self
    }

    pub fn unit(mut self, key: impl Into<Cow<'static, str>>) -> Self {
        self.extra_mut().unit = Some(key.into());
        self
    }

    /// Edited on its own screen (see [`SettingSpec::editor`]).
    pub fn editor(mut self, screen: Id) -> Self {
        self.extra_mut().editor = Some(screen);
        self
    }
}

/// How often a setting is needed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// What most readers change (font size, language, …).
    #[default]
    Basic,
    /// Rarely needed; shown when the user asks for advanced settings.
    Advanced,
}

/// A group of settings, shown together on one page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingGroupSpec {
    pub id: Id,
    /// Translation key of the group's name.
    pub title: Cow<'static, str>,
    /// Groups are listed by order, smallest first.
    pub order: i32,
    /// A screen of the group's plugin shown instead of the generated page
    /// (opened with [`params::ID`](crate::builtin::params::ID), the
    /// group), e.g. Wi-Fi with its networks; it shows the group's
    /// settings itself.
    pub page: Option<Id>,
}

/// Why a setting could not be changed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingError {
    Unknown,
    InvalidValue,
    /// No place is left to keep a changed spec (more than 32,768
    /// settings of a kind).
    Full,
}

impl fmt::Display for SettingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SettingError::Unknown => "unknown setting",
            SettingError::InvalidValue => "invalid value for this setting",
            SettingError::Full => "no place left to change this setting",
        })
    }
}

impl core::error::Error for SettingError {}

/// The settings' specs in registration order, found by id through an
/// index sorted by it: each id kept once. Specs registered as fixed
/// ([`SettingSpec::fixed`], kept in the program, e.g. in flash) are held
/// by reference, the others as they are.
#[derive(Debug, Default)]
struct Specs {
    owned: Vec<SettingSpec>,
    fixed: Vec<&'static SettingSpec>,
    /// Every spec in registration order: a place in `owned`, or in
    /// `fixed` with [`FIXED`] set.
    order: Vec<u16>,
    /// The same places, sorted by the specs' ids.
    by_id: Vec<u16>,
}

/// Marks a place in [`Specs::fixed`].
const FIXED: u16 = 1 << 15;
/// Specs of each kind (made at run time, kept in the program) a place of
/// 15 bits holds: plenty, and two bytes a place on small devices.
const PLACES: usize = FIXED as usize;

impl Specs {
    fn at(&self, place: u16) -> &SettingSpec {
        if place & FIXED == 0 {
            &self.owned[usize::from(place)]
        } else {
            self.fixed[usize::from(place & !FIXED)]
        }
    }

    /// Where `id` is in `by_id`, or would go.
    fn find(&self, id: &str) -> Result<usize, usize> {
        self.by_id
            .binary_search_by(|&at| self.at(at).id.as_str().cmp(id))
    }

    fn get(&self, id: &str) -> Option<&SettingSpec> {
        let at = self.find(id).ok()?;
        Some(self.at(self.by_id[at]))
    }

    /// The spec `id` to change: a fixed one is copied first, which needs
    /// a place of its own.
    fn get_mut(&mut self, id: &str) -> Result<&mut SettingSpec, SettingError> {
        let at = self.find(id).map_err(|_| SettingError::Unknown)?;
        let place = self.by_id[at];
        let place = if place & FIXED == 0 {
            place
        } else if self.owned.len() >= PLACES {
            return Err(SettingError::Full);
        } else {
            let copy = self.fixed[usize::from(place & !FIXED)].clone();
            let owned = self.owned.len() as u16;
            self.owned.push(copy);
            self.by_id[at] = owned;
            if let Some(o) = self.order.iter_mut().find(|o| **o == place) {
                *o = owned;
            }
            owned
        };
        Ok(&mut self.owned[usize::from(place)])
    }

    /// Adds `place`, whose spec's id is not there yet.
    fn index(&mut self, id: &str, place: u16) {
        let Err(at) = self.find(id) else {
            return;
        };
        self.by_id.insert(at, place);
        self.order.push(place);
    }

    /// Adds `spec`, whose id is not there yet; `false` when no place is
    /// left.
    fn insert(&mut self, spec: SettingSpec) -> bool {
        if self.find(spec.id.as_str()).is_ok() {
            return true;
        }
        if self.owned.len() >= PLACES {
            return false;
        }
        let place = self.owned.len() as u16;
        let id = spec.id.clone();
        self.owned.push(spec);
        self.index(id.as_str(), place);
        true
    }

    /// Adds the fixed `spec`, whose id is not there yet; `false` when no
    /// place is left.
    fn insert_fixed(&mut self, spec: &'static SettingSpec) -> bool {
        if self.find(spec.id.as_str()).is_ok() {
            return true;
        }
        if self.fixed.len() >= PLACES {
            return false;
        }
        let place = self.fixed.len() as u16 | FIXED;
        self.fixed.push(spec);
        self.index(spec.id.as_str(), place);
        true
    }

    fn iter(&self) -> impl Iterator<Item = &SettingSpec> {
        self.order.iter().map(|&place| self.at(place))
    }

    fn shrink_to_fit(&mut self) {
        self.owned.shrink_to_fit();
        self.fixed.shrink_to_fit();
        self.order.shrink_to_fit();
        self.by_id.shrink_to_fit();
    }
}

/// All registered settings and their current values.
///
/// Plugins receive it read-only through [`Env`](crate::Env) and change values
/// with [`Command::SetSetting`](crate::Command::SetSetting).
///
/// Something placed more than once, such as a widget on two pages, has
/// values of its own for each placement: the setting `widgets.clock.face`
/// of the placement `widgets.clock@2` is `widgets.clock@2.face` (see
/// [`Id::placement`]), with the spec of `widgets.clock.face`; it is set,
/// saved and reset like any other, and where it is not set the setting's
/// own value applies. While a placement is shown, its values are those of
/// the setting itself ([`Settings::with_placement`]), so a widget reads
/// its settings as always.
///
/// Values may be shown for a while without being kept, e.g. a layout
/// before it is chosen ([`Settings::set_preview`], asked for with
/// [`Command::PreviewSettings`](crate::Command::PreviewSettings)): they
/// are what [`Settings::get`] gives while they last; what is kept
/// ([`Settings::global`]) stays as it is. The ready-made sets of values
/// plugins offer are kept here too ([`Settings::presets`]).
#[derive(Debug, Default)]
pub struct Settings {
    specs: Specs,
    values: VecMap<Id, SettingValue>,
    /// The placement whose values its settings have now, e.g.
    /// `widgets.clock@2` (see [`Settings::with_placement`]).
    placement: core::cell::RefCell<Option<String>>,
    /// Groups by order.
    groups: Vec<SettingGroupSpec>,
    /// The values of the active scope (e.g. the open book), if any.
    scope: Option<VecMap<Id, SettingValue>>,
    /// Values shown without being kept; `None`: as if not set.
    preview: VecMap<Id, Option<SettingValue>>,
    /// Values a power mode wears over the reader's own, while it is worn
    /// (see [`Settings::wear`]).
    worn: VecMap<Id, SettingValue>,
    /// Upper limits of numbers while a power mode is worn.
    limits: VecMap<Id, SettingValue>,
    /// The mode's values the reader changed while it was worn: theirs
    /// until the next mode (see [`Settings::released`]).
    released: Vec<Id>,
    presets: crate::presets::Presets,
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Releases unused capacity once registration is complete.
    pub fn shrink_to_fit(&mut self) {
        self.specs.shrink_to_fit();
        self.values.shrink_to_fit();
        self.groups.shrink_to_fit();
        self.presets.shrink_to_fit();
    }

    /// The presets every plugin added (see [`presets`](crate::presets)).
    pub fn presets(&self) -> &crate::presets::Presets {
        &self.presets
    }

    /// Adds presets (see [`presets`](crate::presets)).
    pub fn presets_mut(&mut self) -> &mut crate::presets::Presets {
        &mut self.presets
    }

    /// Shows `values` in place of those kept (`None`: as if not set),
    /// until they are replaced; empty, the kept ones show again. Values of
    /// unknown settings and those that do not fit are left out.
    pub fn set_preview(&mut self, values: Vec<(Id, Option<SettingValue>)>) {
        self.preview = VecMap::default();
        for (id, value) in values {
            let fits = self
                .spec(id.as_str())
                .is_some_and(|spec| value.as_ref().is_none_or(|value| spec.kind.accepts(value)));
            if fits {
                self.preview.insert(id, value);
            }
        }
    }

    /// Wears a power mode's values over the reader's own, and keeps
    /// numbers below its limits, until another mode is worn (see
    /// [`power`](crate::power)): nothing is written, and the reader's own
    /// values show again when the mode is taken off. Values of unknown
    /// settings and those that do not fit are left out. The ids whose
    /// shown value may have changed come back.
    pub fn wear(&mut self, values: Vec<(Id, SettingValue)>, limits: Vec<(Id, i64)>) -> Vec<Id> {
        let mut touched: Vec<Id> = self
            .worn
            .keys()
            .chain(self.limits.keys())
            .cloned()
            .collect();
        self.worn = VecMap::default();
        self.limits = VecMap::default();
        self.released.clear();
        for (id, value) in values {
            if self
                .spec(id.as_str())
                .is_some_and(|spec| spec.kind.accepts(&value))
            {
                touched.push(id.clone());
                self.worn.insert(id, value);
            }
        }
        for (id, limit) in limits {
            let value = SettingValue::Int(limit);
            if self
                .spec(id.as_str())
                .is_some_and(|spec| spec.kind.accepts(&value))
            {
                touched.push(id.clone());
                self.limits.insert(id, value);
            }
        }
        touched.sort();
        touched.dedup();
        touched
    }

    /// The mode's values the reader changed while it was worn, which are
    /// theirs until the next mode, e.g. to keep across a restart.
    pub fn released(&self) -> &[Id] {
        &self.released
    }

    /// Lets go of the worn mode's value for `id`, as when the reader
    /// changed it (e.g. after a restart, see [`Settings::released`]).
    pub fn release(&mut self, id: &Id) {
        let value = self.worn.remove(id.as_str()).is_some();
        let limit = self.limits.remove(id.as_str()).is_some();
        if (value || limit) && !self.released.contains(id) {
            self.released.push(id.clone());
        }
    }

    /// Whether a power mode holds `id` (a value or a limit).
    pub fn is_worn(&self, id: &str) -> bool {
        self.worn.contains_key(id) || self.limits.contains_key(id)
    }

    /// Whether values are shown without being kept.
    pub fn previewing(&self) -> bool {
        !self.preview.is_empty()
    }

    /// Whether a value is kept for `id` (it is not at its default).
    pub fn is_set(&self, id: &str) -> bool {
        self.values.contains_key(id)
    }

    /// Registers a group of settings.
    pub fn add_group(&mut self, group: SettingGroupSpec) -> Result<(), RegisterError> {
        if self.groups.iter().any(|g| g.id == group.id) {
            return Err(RegisterError::Duplicate { id: group.id });
        }
        let at = self
            .groups
            .partition_point(|g| (g.order, g.id.as_str()) <= (group.order, group.id.as_str()));
        self.groups.insert(at, group);
        Ok(())
    }

    /// Groups by order.
    pub fn groups(&self) -> impl Iterator<Item = &SettingGroupSpec> {
        self.groups.iter()
    }

    /// The group a setting is shown in: its own if registered, else
    /// [`OTHER`](crate::builtin::groups::OTHER).
    pub fn group_of(&self, spec: &SettingSpec) -> Id {
        if self.groups.iter().any(|g| g.id == spec.group) {
            spec.group.clone()
        } else {
            crate::builtin::groups::OTHER
        }
    }

    /// Registers a setting. Its default must be valid.
    pub fn define(&mut self, spec: SettingSpec) -> Result<(), RegisterError> {
        if self.specs.get(spec.id.as_str()).is_some() {
            return Err(RegisterError::Duplicate { id: spec.id });
        }
        if !spec.kind.accepts(&spec.default) {
            return Err(RegisterError::Invalid {
                message: Cow::Borrowed("default value does not match the setting kind"),
            });
        }
        if !self.specs.insert(spec) {
            return Err(RegisterError::Invalid {
                message: Cow::Borrowed("no place left for another setting"),
            });
        }
        Ok(())
    }

    /// Registers a fixed setting ([`SettingSpec::fixed`]), kept by
    /// reference. Its default must be valid.
    pub fn define_fixed(&mut self, spec: &'static SettingSpec) -> Result<(), RegisterError> {
        if self.specs.get(spec.id.as_str()).is_some() {
            return Err(RegisterError::Duplicate {
                id: spec.id.clone(),
            });
        }
        if !spec.kind.accepts(&spec.default) {
            return Err(RegisterError::Invalid {
                message: Cow::Borrowed("default value does not match the setting kind"),
            });
        }
        if !self.specs.insert_fixed(spec) {
            return Err(RegisterError::Invalid {
                message: Cow::Borrowed("no place left for another setting"),
            });
        }
        Ok(())
    }

    /// Replaces the options of a [`SettingKind::Choice`] setting, e.g. once
    /// all contributions it chooses from are known.
    ///
    /// Whether it did: not for an unknown setting, nor where no place is
    /// left to keep the changed spec ([`SettingError::Full`]).
    pub fn set_choices(&mut self, id: &str, options: impl Into<Choices>) -> bool {
        match self.specs.get_mut(id) {
            Ok(spec) => {
                spec.kind = SettingKind::Choice(options.into());
                true
            }
            Err(_) => false,
        }
    }

    /// Changes a setting's default (e.g. device-specific defaults chosen by
    /// a shell). The user's own values still take precedence.
    pub fn set_default_from_text(&mut self, id: &str, text: &str) -> Result<(), SettingError> {
        let spec = self.specs.get_mut(id)?;
        spec.default = spec.kind.parse(text).ok_or(SettingError::InvalidValue)?;
        Ok(())
    }

    /// The spec of setting `id`; for a placement's (`widgets.clock@2.face`)
    /// the setting's own (`widgets.clock.face`).
    pub fn spec(&self, id: &str) -> Option<&SettingSpec> {
        match crate::id::unplaced(id) {
            Some(plain) => self.specs.get(&plain),
            None => self.specs.get(id),
        }
    }

    /// The id a value of `id` is kept under: the spec's, or the
    /// placement's own.
    fn value_id(&self, id: &str) -> Result<Id, SettingError> {
        let spec = self.spec(id).ok_or(SettingError::Unknown)?;
        if id == spec.id.as_str() {
            Ok(spec.id.clone())
        } else {
            Id::parse(id).map_err(|_| SettingError::Unknown)
        }
    }

    /// Sets a value after validating it.
    pub fn set(&mut self, id: &str, value: SettingValue) -> Result<(), SettingError> {
        let spec = self.spec(id).ok_or(SettingError::Unknown)?;
        if !spec.kind.accepts(&value) {
            return Err(SettingError::InvalidValue);
        }
        let id = self.value_id(id)?;
        // The reader's change wins over a mode's value until the next mode.
        self.release(&id);
        self.values.insert(id, value);
        Ok(())
    }

    /// Makes the values of `placement` (e.g. `widgets.clock@2`, see
    /// [`Id::placement`]) those of its settings while `f` runs: a widget
    /// placed there reads `widgets.clock.face` and gets
    /// `widgets.clock@2.face` where that is set. `None` runs `f` as it is.
    pub fn with_placement<R>(&self, placement: Option<&str>, f: impl FnOnce() -> R) -> R {
        let Some(placement) = placement.filter(|p| p.contains('@')) else {
            return f();
        };
        let before = self.placement.replace(Some(String::from(placement)));
        let out = f();
        self.placement.replace(before);
        out
    }

    /// The value the active placement has for `id`, if it sets one.
    fn placed_value(&self, id: &str) -> Option<&SettingValue> {
        let placement = self.placement.borrow();
        let placed = placement.as_deref()?;
        let plain = &placed[..placed.find('@')?];
        let rest = id.strip_prefix(plain).filter(|r| r.starts_with('.'))?;
        let key = alloc::format!("{placed}{rest}");
        match self.preview.get(key.as_str()) {
            Some(shown) => shown.as_ref(),
            None => self.values.get(key.as_str()),
        }
    }

    /// The ids of the values `placement` sets (e.g. to forget them when it
    /// goes).
    pub fn placement_values(&self, placement: &str) -> Vec<Id> {
        self.values
            .keys()
            .filter(|id| {
                id.as_str()
                    .strip_prefix(placement)
                    .is_some_and(|rest| rest.starts_with('.'))
            })
            .cloned()
            .collect()
    }

    /// Forgets the value set for `id`, so that its default applies again;
    /// where changes of it go to the active scope
    /// ([`Settings::goes_to_scope`]), the scope's value, so that the
    /// global one applies again (for "only this book", the scope's values
    /// all go, as when it is switched off).
    pub fn reset(&mut self, id: &str) -> Result<(), SettingError> {
        let id = self.value_id(id)?;
        if self.goes_to_scope(id.as_str()) {
            if let Some(scope) = self.scope.as_mut() {
                if id == crate::builtin::settings::SCOPE_ONLY {
                    scope.clear();
                } else {
                    scope.remove(id.as_str());
                }
            }
        } else {
            self.values.remove(id.as_str());
        }
        Ok(())
    }

    /// Sets a value from configuration-file text.
    pub fn set_from_text(&mut self, id: &str, text: &str) -> Result<(), SettingError> {
        let spec = self.spec(id).ok_or(SettingError::Unknown)?;
        let value = spec.kind.parse(text).ok_or(SettingError::InvalidValue)?;
        let id = self.value_id(id)?;
        self.values.insert(id, value);
        Ok(())
    }

    /// The current value: the active scope's if it sets one (see
    /// [`Settings::scope`]), else the global value, else the default.
    pub fn get(&self, id: &str) -> Option<&SettingValue> {
        if let Some(value) = self.placed_value(id) {
            return Some(value);
        }
        if let Some(shown) = self.preview.get(id) {
            return match (shown, crate::id::unplaced(id)) {
                (Some(value), _) => Some(value),
                (None, Some(plain)) => self.get(&plain),
                (None, None) => self.specs.get(id).map(|spec| &spec.default),
            };
        }
        if let Some(plain) = crate::id::unplaced(id) {
            return self.values.get(id).or_else(|| self.get(&plain));
        }
        if let Some(value) = self.worn.get(id) {
            return Some(value);
        }
        if let Some(limit @ SettingValue::Int(most)) = self.limits.get(id) {
            let own = self.own(id);
            return match own {
                Some(SettingValue::Int(value)) if value <= most => own,
                _ => Some(limit),
            };
        }
        self.own(id)
    }

    /// The value of `id` without a power mode: the scope's, the reader's,
    /// the one it follows, or the default.
    fn own(&self, id: &str) -> Option<&SettingValue> {
        self.scoped_value(id)
            .or_else(|| self.values.get(id))
            .or_else(|| {
                let other = self.follows(id)?;
                self.scoped_value(other)
                    .or_else(|| self.values.get(other))
                    .or_else(|| self.specs.get(other).map(|spec| &spec.default))
            })
            .or_else(|| self.specs.get(id).map(|spec| &spec.default))
    }

    /// The setting `id` follows until it is set
    /// ([`SettingExtra::follows`]), if any.
    fn follows(&self, id: &str) -> Option<&'static str> {
        self.specs.get(id)?.extra().follows
    }

    /// The value outside any scope, or the default if none was set; for a
    /// placement's, the setting's own where the placement sets none.
    pub fn global(&self, id: &str) -> Option<&SettingValue> {
        if let Some(plain) = crate::id::unplaced(id) {
            return self.values.get(id).or_else(|| self.global(&plain));
        }
        self.values
            .get(id)
            .or_else(|| {
                let other = self.follows(id)?;
                self.values
                    .get(other)
                    .or_else(|| self.specs.get(other).map(|spec| &spec.default))
            })
            .or_else(|| self.specs.get(id).map(|spec| &spec.default))
    }

    /// The active scope's value of `id`, if it applies: the setting is
    /// [`scoped`](SettingSpec::scoped) and the scope is switched on
    /// ([`SCOPE_ONLY`](crate::builtin::settings::SCOPE_ONLY)).
    pub fn scoped_value(&self, id: &str) -> Option<&SettingValue> {
        let scope = self.scope.as_ref()?;
        let only = crate::builtin::settings::SCOPE_ONLY;
        if id != only.as_str() && scope.get(only.as_str()) != Some(&SettingValue::Bool(true)) {
            return None;
        }
        self.specs.get(id).filter(|spec| spec.scoped)?;
        scope.get(id)
    }

    /// The values of the active scope, e.g. of the book being read: scoped
    /// settings changed while it is switched on are kept there. `None`
    /// outside any scope.
    pub fn scope(&self) -> Option<&VecMap<Id, SettingValue>> {
        self.scope.as_ref()
    }

    /// Activates a scope with its values (only valid scoped ones are
    /// kept), or ends it.
    pub fn set_scope(&mut self, values: Option<VecMap<Id, SettingValue>>) {
        self.scope = values.map(|mut values| {
            let ids: Vec<Id> = values.keys().cloned().collect();
            for id in ids {
                let valid = match (self.specs.get(id.as_str()), values.get(id.as_str())) {
                    (Some(spec), Some(value)) => spec.scoped && spec.kind.accepts(value),
                    _ => false,
                };
                if !valid {
                    values.remove(id.as_str());
                }
            }
            values
        });
    }

    /// Sets a scoped setting in the active scope. Switching the scope off
    /// ([`SCOPE_ONLY`](crate::builtin::settings::SCOPE_ONLY)) drops its
    /// values.
    pub fn set_in_scope(&mut self, id: &str, value: SettingValue) -> Result<(), SettingError> {
        let spec = self.specs.get(id).ok_or(SettingError::Unknown)?;
        if !spec.scoped || !spec.kind.accepts(&value) {
            return Err(SettingError::InvalidValue);
        }
        let id = spec.id.clone();
        let scope = self.scope.as_mut().ok_or(SettingError::Unknown)?;
        if id == crate::builtin::settings::SCOPE_ONLY && value == SettingValue::Bool(false) {
            scope.clear();
            return Ok(());
        }
        scope.insert(id, value);
        Ok(())
    }

    /// Whether `id` is set in the active scope rather than globally, e.g.
    /// to mark it in menus.
    pub fn is_scoped(&self, id: &str) -> bool {
        id != crate::builtin::settings::SCOPE_ONLY.as_str() && self.scoped_value(id).is_some()
    }

    /// Whether a change of `id` goes to the active scope.
    pub fn goes_to_scope(&self, id: &str) -> bool {
        let Some(scope) = &self.scope else {
            return false;
        };
        let only = crate::builtin::settings::SCOPE_ONLY;
        let scoped = self.specs.get(id).is_some_and(|spec| spec.scoped);
        scoped
            && (id == only.as_str() || scope.get(only.as_str()) == Some(&SettingValue::Bool(true)))
    }

    /// The value of `setting` (see [`setting`](crate::setting)); the
    /// type's empty value if it is unknown.
    pub fn read<T: crate::setting::SettingType>(
        &self,
        setting: &crate::setting::Setting<T>,
    ) -> T::Value<'_> {
        T::read(self.get(setting.as_str()))
    }

    /// The value of a boolean setting by its id; `false` if unknown.
    pub fn bool(&self, id: &str) -> bool {
        matches!(self.get(id), Some(SettingValue::Bool(true)))
    }

    /// The value of an integer setting; `0` if unknown.
    pub fn int(&self, id: &str) -> i64 {
        match self.get(id) {
            Some(SettingValue::Int(v)) => *v,
            _ => 0,
        }
    }

    /// The value of a text or choice setting; `""` if unknown.
    pub fn text(&self, id: &str) -> &str {
        match self.get(id) {
            Some(SettingValue::Text(v)) => v,
            _ => "",
        }
    }

    /// All settings in registration order: plugins registered first come
    /// first, each in the order it declared its settings.
    pub fn specs(&self) -> impl Iterator<Item = &SettingSpec> {
        self.specs.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_follows_another_until_set_itself() {
        static FOLLOWS: SettingExtra = SettingExtra::new().follows("a.sides");
        let mut s = Settings::new();
        let int = |id: &'static str| {
            SettingSpec::new(
                Id::new_static(id),
                id,
                SettingKind::Int { min: 0, max: 99 },
                SettingValue::Int(4),
            )
            .scoped()
        };
        s.define(int("a.sides")).unwrap();
        s.define(int("a.ends").with_extra(&FOLLOWS)).unwrap();
        s.define(
            SettingSpec::new(
                crate::builtin::settings::SCOPE_ONLY,
                "x",
                SettingKind::Bool,
                SettingValue::Bool(false),
            )
            .scoped(),
        )
        .unwrap();
        s.set("a.sides", SettingValue::Int(8)).unwrap();
        assert_eq!(
            (s.int("a.ends"), s.global("a.ends")),
            (8, Some(&SettingValue::Int(8)))
        );
        // A book's own sides, and so its ends.
        let mut values = VecMap::new();
        values.insert(Id::new_static("a.sides"), SettingValue::Int(2));
        values.insert(
            crate::builtin::settings::SCOPE_ONLY.into(),
            SettingValue::Bool(true),
        );
        s.set_scope(Some(values));
        assert_eq!(s.int("a.ends"), 2);
        s.set_scope(None);
        // Set itself, it keeps its own.
        s.set("a.ends", SettingValue::Int(4)).unwrap();
        assert_eq!(s.int("a.ends"), 4);
        s.set("a.sides", SettingValue::Int(12)).unwrap();
        assert_eq!(s.int("a.ends"), 4);
    }

    #[test]
    fn a_power_mode_wears_values_and_limits_and_takes_them_off() {
        let mut s = Settings::new();
        for id in ["a.sleep", "a.light"] {
            let spec = SettingSpec::new(
                Id::new_static(id),
                id,
                SettingKind::Int { min: 0, max: 100 },
                SettingValue::Int(10),
            );
            s.define(spec).unwrap();
        }
        s.set("a.light", SettingValue::Int(80)).unwrap();
        let touched = s.wear(
            alloc::vec![
                (Id::new_static("a.sleep"), SettingValue::Int(5)),
                (Id::new_static("a.unknown"), SettingValue::Int(1)),
            ],
            alloc::vec![(Id::new_static("a.light"), 50)],
        );
        assert_eq!(
            touched,
            [Id::new_static("a.light"), Id::new_static("a.sleep")]
        );
        assert_eq!(s.int("a.sleep"), 5);
        assert_eq!(s.int("a.light"), 50, "kept below the limit");
        // The reader's change wins over a mode's limit, too.
        s.set("a.light", SettingValue::Int(90)).unwrap();
        assert_eq!(s.int("a.light"), 90, "the reader's own");
        // The reader's change wins over the mode's value.
        s.set("a.sleep", SettingValue::Int(20)).unwrap();
        assert_eq!(s.int("a.sleep"), 20);
        assert!(!s.is_worn("a.light") && !s.is_worn("a.sleep"));
        assert_eq!(
            s.released(),
            [Id::new_static("a.light"), Id::new_static("a.sleep")]
        );
        // Taken off: the reader's own values, nothing written by the mode.
        s.wear(alloc::vec![], alloc::vec![]);
        assert_eq!((s.int("a.sleep"), s.int("a.light")), (20, 90));
        assert!(s.released().is_empty());
    }

    #[test]
    fn scopes_override_scoped_settings_while_switched_on() {
        use crate::builtin::settings::SCOPE_ONLY;
        let mut s = Settings::new();
        let int = |id: &'static str, scoped: bool| {
            let spec = SettingSpec::new(
                Id::new_static(id),
                id,
                SettingKind::Int { min: 0, max: 99 },
                SettingValue::Int(10),
            );
            if scoped { spec.scoped() } else { spec }
        };
        s.define(int("a.size", true)).unwrap();
        s.define(int("a.other", false)).unwrap();
        s.define(
            SettingSpec::new(
                SCOPE_ONLY,
                "x",
                SettingKind::Bool,
                SettingValue::Bool(false),
            )
            .scoped(),
        )
        .unwrap();
        s.set("a.size", SettingValue::Int(12)).unwrap();
        assert!(!s.goes_to_scope("a.size"));

        // A book with its own size, switched on.
        let mut values = VecMap::new();
        values.insert(Id::new_static("a.size"), SettingValue::Int(20));
        values.insert(Id::new_static("a.other"), SettingValue::Int(30));
        values.insert(SCOPE_ONLY.into(), SettingValue::Bool(true));
        s.set_scope(Some(values));
        assert_eq!(s.int("a.size"), 20);
        assert!(s.is_scoped("a.size"));
        // Not scoped: the book cannot change it.
        assert_eq!(s.int("a.other"), 10);
        assert!(s.goes_to_scope("a.size") && !s.goes_to_scope("a.other"));
        s.set_in_scope("a.size", SettingValue::Int(25)).unwrap();
        assert_eq!(
            (s.int("a.size"), s.global("a.size")),
            (25, Some(&SettingValue::Int(12)))
        );

        // Switched off: the general value again, the book's dropped.
        s.set_in_scope(SCOPE_ONLY.as_str(), SettingValue::Bool(false))
            .unwrap();
        assert_eq!(s.int("a.size"), 12);
        assert!(s.scope().unwrap().is_empty());
        // Only the switch goes to the scope now.
        assert!(!s.goes_to_scope("a.size") && s.goes_to_scope(SCOPE_ONLY.as_str()));
        s.set_scope(None);
        assert_eq!(s.int("a.size"), 12);
        assert!(!s.goes_to_scope(SCOPE_ONLY.as_str()));
    }

    fn settings() -> Settings {
        let mut settings = Settings::new();
        settings
            .define(SettingSpec::new(
                Id::new_static("test.count"),
                "test.count",
                SettingKind::Int { min: 1, max: 10 },
                SettingValue::Int(3),
            ))
            .unwrap();
        settings
            .define(SettingSpec::new(
                Id::new_static("test.mode"),
                "test.mode",
                SettingKind::Choice(crate::settings::Choices::Fixed(&["a", "b"])),
                SettingValue::Text(Cow::Borrowed("a")),
            ))
            .unwrap();
        settings
    }

    #[test]
    fn defaults_and_updates() {
        let mut s = settings();
        assert_eq!(s.int("test.count"), 3);
        s.set_from_text("test.count", "7").unwrap();
        assert_eq!(s.int("test.count"), 7);
        assert_eq!(s.text("test.mode"), "a");
        s.set_from_text("test.mode", "b").unwrap();
        assert_eq!(s.text("test.mode"), "b");
    }

    #[test]
    fn rejects_invalid_values() {
        let mut s = settings();
        assert_eq!(
            s.set_from_text("test.count", "11"),
            Err(SettingError::InvalidValue)
        );
        assert_eq!(
            s.set_from_text("test.mode", "c"),
            Err(SettingError::InvalidValue)
        );
        assert_eq!(
            s.set("test.count", SettingValue::Bool(true)),
            Err(SettingError::InvalidValue)
        );
        assert_eq!(
            s.set_from_text("test.nope", "1"),
            Err(SettingError::Unknown)
        );
    }

    #[test]
    fn groups_are_ordered_and_unknown_groups_fall_back() {
        let mut s = settings();
        for (id, order) in [("b.late", 20), ("a.early", 10), ("c.same", 10)] {
            s.add_group(SettingGroupSpec {
                id: Id::parse(id).unwrap(),
                title: Cow::Borrowed(""),
                order,
                page: None,
            })
            .unwrap();
        }
        let ids: Vec<&str> = s.groups().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["a.early", "c.same", "b.late"]);
        let spec = SettingSpec::new(
            Id::new_static("x.y"),
            "",
            SettingKind::Bool,
            SettingValue::Bool(false),
        )
        .group(Id::new_static("missing.group"));
        assert_eq!(s.group_of(&spec), crate::builtin::groups::OTHER);
        assert_eq!(
            s.group_of(&spec.clone().group(Id::new_static("b.late")))
                .as_str(),
            "b.late"
        );
    }

    #[test]
    fn rejects_duplicates_and_bad_defaults() {
        let mut s = settings();
        let spec = SettingSpec::new(
            Id::new_static("test.count"),
            "",
            SettingKind::Bool,
            SettingValue::Bool(false),
        );
        assert!(matches!(
            s.define(spec.clone()),
            Err(RegisterError::Duplicate { .. })
        ));
        let mut bad = spec;
        bad.id = Id::new_static("test.bad");
        bad.default = SettingValue::Int(1);
        assert!(matches!(s.define(bad), Err(RegisterError::Invalid { .. })));
    }

    #[test]
    fn placements_have_values_of_their_own() {
        let mut s = Settings::new();
        static SPECS: [SettingSpec; 1] = [SettingSpec::fixed_switch(
            "widgets.clock.date",
            "widgets.clock.date",
            true,
            Id::new_static("widgets.clock"),
        )];
        s.define_fixed(&SPECS[0]).unwrap();
        // Unset, a placement has the setting's value.
        assert!(s.bool("widgets.clock@2.date"));
        s.set("widgets.clock@2.date", SettingValue::Bool(false))
            .unwrap();
        assert!(!s.bool("widgets.clock@2.date"));
        assert!(s.bool("widgets.clock.date"));
        // Its spec is the setting's: its kind holds.
        assert_eq!(
            s.set("widgets.clock@2.date", SettingValue::Int(1)),
            Err(SettingError::InvalidValue)
        );
        assert_eq!(
            s.spec("widgets.clock@2.date").unwrap().id.as_str(),
            "widgets.clock.date"
        );
        // While the placement is shown, it is the setting's value.
        let date = s.with_placement(Some("widgets.clock@2"), || s.bool("widgets.clock.date"));
        assert!(!date);
        assert!(s.with_placement(Some("widgets.clock@3"), || s.bool("widgets.clock.date")));
        assert!(s.bool("widgets.clock.date"));
        // Read from configuration text, and forgotten again.
        s.set_from_text("widgets.clock@3.date", "off").unwrap();
        assert_eq!(s.placement_values("widgets.clock@3").len(), 1);
        s.reset("widgets.clock@3.date").unwrap();
        assert!(s.bool("widgets.clock@3.date"));
        assert!(s.placement_values("widgets.clock@3").is_empty());
        // Another widget's setting is not the placement's.
        assert_eq!(
            s.set("widgets.other@2.date", SettingValue::Bool(true)),
            Err(SettingError::Unknown)
        );
    }

    #[test]
    fn previews_show_values_without_keeping_them() {
        let mut s = Settings::new();
        static SPECS: [SettingSpec; 2] = [
            SettingSpec::fixed_switch(
                "widgets.clock.date",
                "widgets.clock.date",
                true,
                Id::new_static("widgets.clock"),
            ),
            SettingSpec::fixed_number(
                "widgets.clock.size",
                "",
                1,
                9,
                5,
                Id::new_static("widgets.clock"),
            ),
        ];
        s.define_fixed(&SPECS[0]).unwrap();
        s.define_fixed(&SPECS[1]).unwrap();
        s.set("widgets.clock.size", SettingValue::Int(7)).unwrap();
        s.set("widgets.clock@2.date", SettingValue::Bool(false))
            .unwrap();
        s.set_preview(alloc::vec![
            (
                Id::new_static("widgets.clock.date"),
                Some(SettingValue::Bool(false))
            ),
            (Id::new_static("widgets.clock.size"), None),
            (Id::new_static("widgets.clock@2.date"), None),
            (
                Id::new_static("widgets.clock@3.date"),
                Some(SettingValue::Bool(true))
            ),
            // Unknown, or not fitting: left out.
            (Id::new_static("widgets.none"), None),
            (
                Id::new_static("widgets.clock@4.date"),
                Some(SettingValue::Int(1))
            ),
        ]);
        assert!(s.previewing());
        assert!(!s.bool("widgets.clock.date"));
        // Not set, while shown: the default, not the value kept.
        assert_eq!(s.int("widgets.clock.size"), 5);
        assert_eq!(s.global("widgets.clock.size"), Some(&SettingValue::Int(7)));
        // A placement not set shows the setting's (shown) value.
        assert!(!s.bool("widgets.clock@2.date"));
        assert!(s.with_placement(Some("widgets.clock@3"), || s.bool("widgets.clock.date")));
        assert!(!s.with_placement(Some("widgets.clock@4"), || s.bool("widgets.clock.date")));
        s.set_preview(Vec::new());
        assert!(!s.previewing());
        assert_eq!(s.int("widgets.clock.size"), 7);
        assert!(s.bool("widgets.clock.date"));
        assert!(s.is_set("widgets.clock@2.date"));
        assert!(!s.is_set("widgets.clock.date"));
    }
}
