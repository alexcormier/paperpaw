//! What plugins offer each other: services, and points to contribute to.
//!
//! **Services.** A plugin publishes a value under an id in its namespace
//! ([`Registrar::publish`](crate::Registrar::publish)); others find it
//! with [`Services::get`] by that id and its type. The type comes from
//! the publishing plugin's crate, usually a trait object, so others depend
//! on what it promises, not on how it does it:
//!
//! ```
//! # use paperpaw_sdk::services::{Service, Services};
//! # use paperpaw_sdk::Id;
//! # extern crate alloc;
//! # use alloc::boxed::Box;
//! /// In the publishing plugin's crate.
//! pub trait Weather {
//!     fn today(&self) -> &str;
//! }
//! pub const WEATHER: Id = Id::new_static("weather.service");
//!
//! struct Sunny;
//! impl Weather for Sunny {
//!     fn today(&self) -> &str {
//!         "sunny"
//!     }
//! }
//!
//! // `register` of the publishing plugin:
//! // r.publish(WEATHER, Box::new(Sunny) as Box<dyn Weather>)?;
//! # let mut services = Services::new();
//! # services.insert(WEATHER, Service::new(Box::new(Sunny) as Box<dyn Weather>));
//!
//! // Any plugin, with an `Env`: `env.services.get::<…>(…)`.
//! let weather = services.get::<Box<dyn Weather>>(WEATHER.as_str());
//! assert_eq!(weather.map(|w| w.today()), Some("sunny"));
//! ```
//!
//! A service is shared by all and lives as long as PaperPaw runs; one
//! that changes keeps its state in a `Cell` or `RefCell`. One that is
//! missing (the plugin is switched off) is `None`: plugins that use
//! another's service do without it.
//!
//! A service is found by its type's name as well as by its `TypeId`: a
//! native code plugin (see [`native`](crate::native)) is built apart from
//! the firmware, and the same type has another `TypeId` in each build
//! (cargo's settings for a build are part of it). By its name only where
//! the type is made of the crates under the plugin interface
//! ([`native::CROSSING_CRATES`](crate::native::CROSSING_CRATES), `core`
//! and `alloc`), whose types the interface keeps as they are, and of the
//! same size and alignment: a type of a plugin's own crate may be another
//! of the same name in another build, so it needs a contract.
//!
//! **Contracts.** A service type that implements [`ServiceType`] names
//! its contract in a key that stays the same in every build
//! (`"weather.forecast/1"`): published with
//! [`Registrar::provide`](crate::Registrar::provide), with a version, it is
//! found by that key ([`Services::get_typed`],
//! [`Services::get_versioned`]), its size and alignment checked, so a
//! native plugin built apart finds it safely; a type whose contract
//! changes takes a new key.
//!
//! **Points to contribute to.** A plugin names a point (an id in its
//! namespace) and documents what a contribution there is, e.g. a line
//! of text in a format of its own; any plugin adds to it with
//! [`Registrar::contribute`](crate::Registrar::contribute), and the
//! plugin reads them all with [`Services::contributions`]. The kernel's
//! own points (status items, widgets, text filters …) have registrar
//! methods of their own; these are for points that plugins define.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::{Any, type_name};
use core::fmt;

use crate::{Id, Version};

/// The contract of a service type: a key that names it in every build
/// (`"<plugin>.<name>/<major>"`), so that plugins built apart find it.
/// Implemented by the type that is published, usually a struct holding a
/// trait object:
///
/// ```
/// # use paperpaw_sdk::services::ServiceType;
/// # extern crate alloc;
/// pub trait Forecast {
///     fn today(&self) -> &str;
/// }
/// pub struct Weather(pub alloc::boxed::Box<dyn Forecast>);
/// impl ServiceType for Weather {
///     const KEY: &'static str = "weather.forecast/1";
/// }
/// ```
pub trait ServiceType: Any {
    const KEY: &'static str;
}

/// A service's value, with the name, size and alignment of its type when
/// it was published with [`Registrar::publish`](crate::Registrar::publish),
/// or its contract's key, size and version with
/// [`Registrar::provide`](crate::Registrar::provide).
pub struct Service {
    value: Box<dyn Any>,
    type_name: Option<Named>,
    /// Its contract and version, where it has them (kept aside: most
    /// services and contributions have none).
    contract: Option<Box<Contract>>,
}

/// A service's type by its name: the name, size and alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Named {
    name: &'static str,
    size: usize,
    align: usize,
}

impl Named {
    fn of<T>() -> Self {
        Self {
            name: type_name::<T>(),
            size: core::mem::size_of::<T>(),
            align: core::mem::align_of::<T>(),
        }
    }
}

/// Whether a type of this name is made of types of `core`, `alloc` and the
/// crates under the plugin interface only: then it is the same type in
/// every build of the same interface.
fn under_the_interface(name: &str) -> bool {
    name.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
        .filter_map(|part| part.split_once("::").map(|(krate, _)| krate))
        .all(|krate| {
            matches!(krate, "core" | "alloc" | "std")
                || crate::native::CROSSING_CRATES.contains(&krate)
        })
}

/// A service's contract: its type's key, size and alignment, and the
/// version it was published in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Contract {
    key: &'static str,
    size: usize,
    align: usize,
    version: Version,
}

impl Service {
    /// `value`, found by its type's `TypeId` and name.
    pub fn new<T: Any>(value: T) -> Self {
        Self {
            value: Box::new(value),
            type_name: Some(Named::of::<T>()),
            contract: None,
        }
    }

    /// `value` of a type with a contract ([`ServiceType`]) in `version`:
    /// found by its `TypeId`, or by its contract's key where its size and
    /// alignment are the same.
    pub fn typed<T: ServiceType>(value: T, version: Version) -> Self {
        let contract = Contract {
            key: T::KEY,
            size: core::mem::size_of::<T>(),
            align: core::mem::align_of::<T>(),
            version,
        };
        Self {
            value: Box::new(value),
            type_name: None,
            contract: Some(Box::new(contract)),
        }
    }

    /// Its version, where it was published with one.
    pub fn version(&self) -> Option<Version> {
        self.contract.as_ref().map(|contract| contract.version)
    }

    /// The value, if it is a `T` of the same contract.
    fn get_typed<T: ServiceType>(&self) -> Option<&T> {
        if let Some(value) = self.value.downcast_ref::<T>() {
            return Some(value);
        }
        let (size, align) = (core::mem::size_of::<T>(), core::mem::align_of::<T>());
        let same = self
            .contract
            .as_ref()
            .is_some_and(|c| (c.key, c.size, c.align) == (T::KEY, size, align));
        if !same {
            return None;
        }
        let value: *const dyn Any = &*self.value;
        // SAFETY: the value is of a type with the same contract, size and
        // alignment: the same type from the same source, only in another
        // build (a native plugin's), which gives it another `TypeId`.
        Some(unsafe { &*value.cast::<T>() })
    }

    /// The value, if it is a `T`.
    fn get<T: Any>(&self) -> Option<&T> {
        if let Some(value) = self.value.downcast_ref::<T>() {
            return Some(value);
        }
        let named = Named::of::<T>();
        if self.type_name != Some(named) || !under_the_interface(named.name) {
            return None;
        }
        let value: *const dyn Any = &*self.value;
        // SAFETY: the value is of a type of this name, size and alignment,
        // made of types of the crates under the plugin interface only: of
        // `T`, as the interface keeps it (a native plugin's fingerprint
        // says the same interface, and its compiler the firmware's), only
        // in another build, which gives it another `TypeId`.
        Some(unsafe { &*value.cast::<T>() })
    }
}

impl fmt::Debug for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Service")
            .field("type_name", &self.type_name)
            .field("contract", &self.contract)
            .finish_non_exhaustive()
    }
}

/// A point plugins contribute to, as its plugin declares it
/// ([`Registrar::add_point`](crate::Registrar::add_point)): for lists of
/// what can be extended, and for authors.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PointSpec {
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    /// What a contribution there is, its format (for authors).
    pub doc: Cow<'static, str>,
}

impl PointSpec {
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id,
            title: title.into(),
            doc: Cow::Borrowed(""),
        }
    }

    pub fn doc(mut self, doc: impl Into<Cow<'static, str>>) -> Self {
        self.doc = doc.into();
        self
    }
}

/// An event a plugin emits, as it declares it
/// ([`Registrar::declare_event`](crate::Registrar::declare_event)): rules
/// and macro editors offer any declared event.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EventSpec {
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    /// The names of the params it carries.
    pub params: &'static [&'static str],
}

impl EventSpec {
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id,
            title: title.into(),
            params: &[],
        }
    }

    pub fn params(mut self, params: &'static [&'static str]) -> Self {
        self.params = params;
        self
    }
}

/// Every service and contribution; see the [module documentation](self).
#[derive(Default)]
pub struct Services {
    services: Vec<(Id, Service)>,
    /// Point, plugin, contribution.
    contributions: Vec<(Id, Id, Cow<'static, str>)>,
    points: Vec<PointSpec>,
    events: Vec<EventSpec>,
}

/// The kernel's services a plugin may stand in for, publishing its own
/// under the same id ([`Registrar::publish`](crate::Registrar::publish)):
/// the library ([`library::service`](fn@crate::library::service)).
pub const REPLACEABLE: &[&str] = &[crate::library::service::SERVICE];

impl Services {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publishes `service` as `id` (kernel; plugins use
    /// [`Registrar::publish`](crate::Registrar::publish)).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn insert(&mut self, id: Id, service: Service) {
        self.services.push((id, service));
    }

    /// Adds `plugin`'s `contribution` to `point` (kernel; plugins use
    /// [`Registrar::contribute`](crate::Registrar::contribute)).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn contribute(&mut self, point: Id, plugin: Id, contribution: Cow<'static, str>) {
        self.contributions.push((point, plugin, contribution));
    }

    /// Declares point `point` (kernel; plugins use
    /// [`Registrar::add_point`](crate::Registrar::add_point)).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn declare_point(&mut self, point: PointSpec) {
        self.points.push(point);
    }

    /// Declares event `event` (kernel; plugins use
    /// [`Registrar::declare_event`](crate::Registrar::declare_event)).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn declare_event(&mut self, event: EventSpec) {
        self.events.push(event);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.services.shrink_to_fit();
        self.contributions.shrink_to_fit();
        self.points.shrink_to_fit();
        self.events.shrink_to_fit();
    }

    /// Service `id` of contract `T` ([`ServiceType`]), if it is published.
    pub fn get_typed<T: ServiceType>(&self, id: &str) -> Option<&T> {
        self.services
            .iter()
            .find(|(s, _)| s.as_str() == id)
            .and_then(|(_, service)| service.get_typed::<T>())
    }

    /// Service `id` of contract `T` in a version that satisfies `min`
    /// (as a plugin's SDK version does: the same major, not older).
    pub fn get_versioned<T: ServiceType>(&self, id: &str, min: Version) -> Option<&T> {
        self.services
            .iter()
            .find(|(s, service)| {
                s.as_str() == id && service.version().is_some_and(|v| v.satisfies(&min))
            })
            .and_then(|(_, service)| service.get_typed::<T>())
    }

    /// The ids of every service published.
    pub fn ids(&self) -> impl Iterator<Item = &Id> {
        self.services.iter().map(|(id, _)| id)
    }

    /// Every point declared to contribute to.
    pub fn points(&self) -> &[PointSpec] {
        &self.points
    }

    /// Every event declared.
    pub fn events(&self) -> &[EventSpec] {
        &self.events
    }

    /// Service `id`, if it is published and of type `T`.
    pub fn get<T: Any>(&self, id: &str) -> Option<&T> {
        self.services
            .iter()
            .find(|(s, _)| s.as_str() == id)
            .and_then(|(_, service)| service.get::<T>())
    }

    /// Whether service `id` is published.
    pub fn has(&self, id: &str) -> bool {
        self.services.iter().any(|(s, _)| s.as_str() == id)
    }

    /// The points something was contributed to, each once, in the order
    /// of their first contribution (declared or not, see
    /// [`Services::points`]).
    pub fn contributed_points(&self) -> impl Iterator<Item = &Id> {
        let all = &self.contributions;
        all.iter()
            .enumerate()
            .filter(move |(at, (point, _, _))| !all[..*at].iter().any(|(p, _, _)| p == point))
            .map(|(_, (point, _, _))| point)
    }

    /// The contributions to `point`, with the plugin each comes from, in
    /// the order the plugins registered.
    pub fn contributions<'a>(&'a self, point: &'a str) -> impl Iterator<Item = (&'a Id, &'a str)> {
        self.contributions
            .iter()
            .filter(move |(p, _, _)| p.as_str() == point)
            .map(|(_, plugin, text)| (plugin, text.as_ref()))
    }
}

impl fmt::Debug for Services {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Services")
            .field(
                "services",
                &self.services.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            )
            .field("contributions", &self.contributions.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_and_points_are_listed_each_once() {
        let mut services = Services::new();
        services.insert(Id::new_static("a.count"), Service::new(3u32));
        services.insert(Id::new_static("b.name"), Service::new("x"));
        let ids: Vec<&str> = services.ids().map(Id::as_str).collect();
        assert_eq!(ids, ["a.count", "b.name"]);
        let (a, b) = (Id::new_static("a"), Id::new_static("b"));
        services.contribute(Id::new_static("p.one"), a.clone(), Cow::Borrowed("1"));
        services.contribute(Id::new_static("p.two"), b.clone(), Cow::Borrowed("2"));
        services.contribute(Id::new_static("p.one"), b, Cow::Borrowed("3"));
        let points: Vec<&str> = services.contributed_points().map(Id::as_str).collect();
        assert_eq!(points, ["p.one", "p.two"]);
        assert_eq!(Services::new().contributed_points().count(), 0);
    }

    #[test]
    fn services_by_id_and_type() {
        let mut services = Services::new();
        services.insert(Id::new_static("a.count"), Service::new(3u32));
        assert_eq!(services.get::<u32>("a.count"), Some(&3));
        assert_eq!(services.get::<i64>("a.count"), None, "another type");
        assert_eq!(services.get::<u32>("b.count"), None);
        assert!(services.has("a.count"));

        let point = Id::new_static("a.shapes");
        services.contribute(point.clone(), Id::new_static("b"), Cow::Borrowed("circle"));
        services.contribute(
            Id::new_static("a.other"),
            Id::new_static("b"),
            Cow::Borrowed("x"),
        );
        services.contribute(point, Id::new_static("c"), Cow::Borrowed("square"));
        let shapes: Vec<(&str, &str)> = services
            .contributions("a.shapes")
            .map(|(plugin, text)| (plugin.as_str(), text))
            .collect();
        assert_eq!(shapes, [("b", "circle"), ("c", "square")]);
    }

    /// The same type built apart from the publisher: the same name and
    /// layout, another `TypeId`.
    #[test]
    fn services_built_apart_are_found_by_their_type_name() {
        #[derive(Debug, PartialEq)]
        #[repr(C)]
        struct Forecast(u32);
        #[repr(C)]
        struct BuiltApart(u32);
        let mut services = Services::new();
        let apart = Service {
            value: Box::new(BuiltApart(7)),
            type_name: Some(Named::of::<Forecast>()),
            contract: None,
        };
        services.insert(Id::new_static("w.forecast"), apart);
        assert_eq!(services.get::<Forecast>("w.forecast"), Some(&Forecast(7)));
        assert_eq!(services.get::<u32>("w.forecast"), None);
    }

    /// By its name only a type of the crates under the interface: one of a
    /// plugin's own crate may be another type in another build.
    #[test]
    fn only_types_under_the_interface_are_found_by_name() {
        for name in [
            "u32",
            "&str",
            "alloc::boxed::Box<dyn paperpaw_sdk::library::Library>",
            "core::option::Option<paperpaw_platform::input::Instant>",
            "(u8, paperpaw_document::Block)",
        ] {
            assert!(under_the_interface(name), "{name}");
        }
        for name in [
            "weather::Forecast",
            "alloc::boxed::Box<dyn weather::Forecast>",
            "alloc::vec::Vec<paperpaw_plugin_stats::Day>",
            "paperpaw_sdk::Id2<other::Thing>",
        ] {
            assert!(!under_the_interface(name), "{name}");
        }
    }

    /// The same, found by its contract: the key, size and alignment.
    #[test]
    fn services_built_apart_are_found_by_their_contract() {
        #[derive(Debug, PartialEq)]
        #[repr(C)]
        struct Forecast(u32);
        impl ServiceType for Forecast {
            const KEY: &'static str = "w.forecast/1";
        }
        #[repr(C)]
        struct BuiltApart(u32);
        let mut services = Services::new();
        let apart = Service {
            value: Box::new(BuiltApart(7)),
            type_name: None,
            contract: Some(Box::new(Contract {
                key: Forecast::KEY,
                size: 4,
                align: 4,
                version: Version::new(1, 0, 0),
            })),
        };
        services.insert(Id::new_static("w.forecast"), apart);
        assert_eq!(
            services.get_typed::<Forecast>("w.forecast"),
            Some(&Forecast(7))
        );
        let old = Version::new(1, 0, 0);
        assert!(
            services
                .get_versioned::<Forecast>("w.forecast", old)
                .is_some()
        );
    }
}
