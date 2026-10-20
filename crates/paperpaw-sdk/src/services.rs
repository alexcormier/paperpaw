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
//! (cargo's settings for a build are part of it). The name is the same
//! where the fingerprint is, so a service whose type changes takes a new
//! id (`weather.forecast-2`), as the type's name would not tell.
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

use crate::Id;

/// A service's value, with the name of its type when it was published
/// with [`Registrar::publish`](crate::Registrar::publish).
pub struct Service {
    value: Box<dyn Any>,
    type_name: Option<&'static str>,
}

impl Service {
    /// `value`, found by its type's `TypeId` and name.
    pub fn new<T: Any>(value: T) -> Self {
        Self {
            value: Box::new(value),
            type_name: Some(type_name::<T>()),
        }
    }

    /// A boxed value of a type not known here, found by its `TypeId` only
    /// (so not by native code plugins built apart).
    pub fn any(value: Box<dyn Any>) -> Self {
        Self {
            value,
            type_name: None,
        }
    }

    /// The value, if it is a `T`.
    fn get<T: Any>(&self) -> Option<&T> {
        if let Some(value) = self.value.downcast_ref::<T>() {
            return Some(value);
        }
        if self.type_name != Some(type_name::<T>()) {
            return None;
        }
        let value: *const dyn Any = &*self.value;
        // SAFETY: the value is of a type of this name: of `T`, as built by
        // the same compiler from the same source (a native plugin's
        // fingerprint says so), only in another build, which gives it
        // another `TypeId`.
        Some(unsafe { &*value.cast::<T>() })
    }
}

impl fmt::Debug for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Service")
            .field("type_name", &self.type_name)
            .finish_non_exhaustive()
    }
}

/// Every service and contribution; see the [module documentation](self).
#[derive(Default)]
pub struct Services {
    services: Vec<(Id, Service)>,
    /// Point, plugin, contribution.
    contributions: Vec<(Id, Id, Cow<'static, str>)>,
}

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

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.services.shrink_to_fit();
        self.contributions.shrink_to_fit();
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

    /// The ids of the services published, in the order they were (e.g.
    /// for a list of what plugins offer each other).
    pub fn ids(&self) -> impl Iterator<Item = &Id> {
        self.services.iter().map(|(id, _)| id)
    }

    /// The points something was contributed to, each once, in the order
    /// of their first contribution.
    pub fn points(&self) -> impl Iterator<Item = &Id> {
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
        let points: Vec<&str> = services.points().map(Id::as_str).collect();
        assert_eq!(points, ["p.one", "p.two"]);
        assert_eq!(Services::new().points().count(), 0);
    }

    #[test]
    fn services_by_id_and_type() {
        let mut services = Services::new();
        services.insert(Id::new_static("a.count"), Service::new(3u32));
        assert_eq!(services.get::<u32>("a.count"), Some(&3));
        assert_eq!(services.get::<i64>("a.count"), None, "another type");
        assert_eq!(services.get::<u32>("b.count"), None);
        assert!(services.has("a.count"));
        services.insert(Id::new_static("a.any"), Service::any(Box::new(5u8)));
        assert_eq!(services.get::<u8>("a.any"), Some(&5), "by its TypeId");

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
            type_name: Some(type_name::<Forecast>()),
        };
        services.insert(Id::new_static("w.forecast"), apart);
        assert_eq!(services.get::<Forecast>("w.forecast"), Some(&Forecast(7)));
        assert_eq!(services.get::<u32>("w.forecast"), None);
    }
}
