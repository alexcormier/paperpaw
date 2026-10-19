//! Code plugins (ADR 0024): folders on the storage like data plugins'
//! (`plugin.ini`, translations, settings) with native code beside them
//! (`code.ppn` on the X4 and the X4 Pro). The shell loads the code
//! ([`CodeLoader`], given with
//! [`KernelBuilder::code_plugins`](crate::KernelBuilder::code_plugins));
//! the kernel decides which, at the start only, as the configuration
//! says, and registers each with its folder's data. Switching one takes a
//! restart.
//!
//! Safe mode ([`KernelBuilder::safe_mode`](crate::KernelBuilder::safe_mode))
//! starts without code plugins and switches off the one the last crash was
//! in, or, when that is not known, every one that was on: each stays off
//! until it is switched on again.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;
use paperpaw_sdk::native::{Native, SafeMode, SafeModeReason};
use paperpaw_sdk::{
    Id, Manifest, Plugin, PluginKind, PluginProblem, PluginState, RegisterError, Registrar,
};

use crate::data_plugin::DataPlugin;
use crate::{Kernel, WarningSource};

/// What loads the code of code plugins: the shell's, which knows where it
/// can run.
pub trait CodeLoader {
    /// Loads the code in the file at `path` (a plugin folder's
    /// [`Native::file`]) and makes its plugin, or says why it cannot. Called
    /// at the start, once for every code plugin switched on, in the order
    /// of their ids.
    fn load(&mut self, storage: &dyn Storage, path: &str)
    -> Result<Box<dyn Plugin>, PluginProblem>;

    /// Every code plugin of this start is loaded.
    fn finish(&mut self, _storage: &dyn Storage) {}
}

/// How the kernel loads code plugins at this start.
pub(crate) struct CodePlugins {
    pub(crate) native: Native,
    pub(crate) loader: Box<dyn CodeLoader>,
    /// This start goes without them.
    pub(crate) safe_mode: Option<SafeMode>,
}

/// A code plugin: its code, then its folder's data.
struct WithData {
    code: Box<dyn Plugin>,
    data: DataPlugin,
}

impl Plugin for WithData {
    fn manifest(&self) -> Manifest {
        self.code.manifest()
    }

    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError> {
        self.code.register(registrar)?;
        self.data.register(registrar)
    }
}

impl Kernel {
    /// Registers the code plugins found among the storage's plugins, in
    /// the order of their ids, as the configuration (`disabled`) and safe
    /// mode say.
    pub(crate) fn add_code_plugins(
        &mut self,
        mut found: Vec<DataPlugin>,
        disabled: &[String],
        code: Option<CodePlugins>,
    ) {
        found.sort_by(|a, b| a.manifest().id.cmp(&b.manifest().id));
        let Some(CodePlugins {
            mut native,
            mut loader,
            safe_mode,
        }) = code
        else {
            for plugin in found {
                self.list_code_plugin(
                    &plugin,
                    disabled,
                    PluginState::Failed,
                    PluginProblem::Unsupported,
                );
            }
            return;
        };
        if let Some(mut safe_mode) = safe_mode {
            self.hold_back(found, disabled, &mut safe_mode);
            native.safe_mode = Some(safe_mode);
            self.shared.native = Some(native);
            return;
        }
        let storage = self.shared.storage.clone();
        for plugin in found {
            let manifest = plugin.manifest();
            let id = manifest.id.clone();
            if disabled.iter().any(|d| d == id.as_str()) {
                self.register_off(&plugin, manifest);
                self.set_code_info(&id, None);
                continue;
            }
            let path = native.path(plugin.folder());
            let loaded = loader.load(storage.as_ref(), &path).and_then(|code| {
                if code.manifest().id == id {
                    Ok(code)
                } else {
                    Err(PluginProblem::Mismatch)
                }
            });
            match loaded {
                Ok(code) => {
                    let plugin = WithData { code, data: plugin };
                    let manifest = plugin.manifest();
                    self.register_plugin(&plugin, manifest);
                    self.set_code_info(&id, None);
                }
                Err(problem) => {
                    let message = format!("{path}: not loaded ({problem:?})");
                    self.warn(WarningSource::Plugin(id.clone()), None, message);
                    self.list_code_plugin(&plugin, disabled, PluginState::Failed, problem);
                }
            }
        }
        loader.finish(storage.as_ref());
        self.shared.native = Some(native);
    }

    /// A code plugin installed while PaperPaw runs (`Command::LoadPlugin`):
    /// listed, its code loaded at the next start.
    pub(crate) fn add_code_plugin_later(&mut self, plugin: &DataPlugin, off: bool) {
        let disabled: Vec<String> = if off {
            alloc::vec![String::from(plugin.manifest().id.as_str())]
        } else {
            Vec::new()
        };
        let problem = match &self.shared.native {
            Some(_) => None,
            None => Some(PluginProblem::Unsupported),
        };
        let state = if problem.is_some() {
            PluginState::Failed
        } else {
            PluginState::Off
        };
        self.list_code_plugin(plugin, &disabled, state, PluginProblem::Unsupported);
        self.set_code_info(&plugin.manifest().id, problem);
    }

    /// Safe mode: no code is loaded; the plugin the crash was in is
    /// switched off, or, when that is not known, every one that was on.
    fn hold_back(&mut self, found: Vec<DataPlugin>, disabled: &[String], safe_mode: &mut SafeMode) {
        let culprit = match safe_mode.reason {
            SafeModeReason::FailedStarts => safe_mode.crashed.clone(),
            _ => None,
        };
        for plugin in found {
            let id = plugin.manifest().id;
            let on = !disabled.iter().any(|d| d == id.as_str());
            let switch_off = on && culprit.as_ref().is_none_or(|c| c == id.as_str());
            if switch_off {
                self.save_config("plugins", id.as_str(), "off");
                safe_mode.switched_off.push(id.clone());
            }
            let problem = if culprit.is_some() && switch_off {
                PluginProblem::Crashed
            } else {
                PluginProblem::SafeMode
            };
            let disabled: Vec<String> = if switch_off {
                alloc::vec![String::from(id.as_str())]
            } else {
                disabled.to_vec()
            };
            self.list_code_plugin(&plugin, &disabled, PluginState::Off, problem);
        }
    }

    /// Lists a code plugin whose code is not loaded, in `state` (when
    /// switched on) and why: its translations only, as for one switched
    /// off.
    fn list_code_plugin(
        &mut self,
        plugin: &DataPlugin,
        disabled: &[String],
        state: PluginState,
        problem: PluginProblem,
    ) {
        let manifest = plugin.manifest();
        let id = manifest.id.clone();
        self.register_off(plugin, manifest);
        let on = !disabled.iter().any(|d| d == id.as_str());
        if let Some(info) = self.shared.plugins.get_mut(id.as_str()) {
            info.enabled = on;
            if on {
                info.state = state;
            }
        }
        self.set_code_info(&id, Some(problem));
    }

    fn set_code_info(&mut self, id: &Id, problem: Option<PluginProblem>) {
        if let Some(info) = self.shared.plugins.get_mut(id.as_str()) {
            info.kind = PluginKind::Code;
            info.problem = problem;
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::borrow::Cow;
    use alloc::rc::Rc;
    use core::cell::RefCell;

    use paperpaw_sdk::{ActionSpec, MemoryStorage, SDK_VERSION, Version};

    use super::*;
    use crate::tests::DEVICE;

    /// A plugin whose code the loader "loaded": one action.
    struct Code(&'static str);

    impl Plugin for Code {
        fn manifest(&self) -> Manifest {
            Manifest {
                id: Id::new_static(self.0),
                name: Cow::Borrowed("Code"),
                description: Cow::Borrowed("Native code."),
                version: Version::new(1, 0, 0),
                sdk: SDK_VERSION,
            }
        }

        fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
            let id =
                Id::parse(&format!("{}.hello", self.0)).map_err(|_| RegisterError::Invalid {
                    message: Cow::Borrowed("id"),
                })?;
            r.add_action(ActionSpec::new(id, "hello"))
        }
    }

    /// Loads from a table, noting what it was asked.
    #[derive(Clone, Default)]
    struct Loader {
        asked: Rc<RefCell<Vec<String>>>,
        finished: Rc<RefCell<bool>>,
        answers: Vec<(&'static str, Result<&'static str, PluginProblem>)>,
    }

    impl CodeLoader for Loader {
        fn load(&mut self, _: &dyn Storage, path: &str) -> Result<Box<dyn Plugin>, PluginProblem> {
            self.asked.borrow_mut().push(String::from(path));
            let answer = self
                .answers
                .iter()
                .find(|(p, _)| *p == path)
                .map(|(_, a)| *a);
            match answer {
                Some(Ok(id)) => Ok(Box::new(Code(id))),
                Some(Err(problem)) => Err(problem),
                None => Err(PluginProblem::Damaged),
            }
        }

        fn finish(&mut self, _: &dyn Storage) {
            *self.finished.borrow_mut() = true;
        }
    }

    fn storage() -> MemoryStorage {
        MemoryStorage::new()
            .with("plugins/zeta/plugin.ini", "[plugin]\nname = Zeta\n")
            .with("plugins/zeta/code.ppn", "PPNP")
            .with("plugins/hello/plugin.ini", "[plugin]\nname = Hello\n")
            .with("plugins/hello/code.ppn", "PPNP")
            .with(
                "plugins/hello/i18n/en.ini",
                "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\nhello.greeting = Hi\n",
            )
            .with("plugins/theme/plugin.ini", "[plugin]\nname = Theme\n")
    }

    fn loader() -> Loader {
        Loader {
            answers: alloc::vec![
                ("plugins/hello/code.ppn", Ok("hello")),
                ("plugins/zeta/code.ppn", Ok("zeta")),
            ],
            ..Loader::default()
        }
    }

    fn kernel(loader: &Loader, config: &str, safe_mode: Option<SafeMode>) -> Kernel {
        let builder = Kernel::builder(DEVICE)
            .storage(storage())
            .user_config(config)
            .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()));
        match safe_mode {
            Some(safe_mode) => builder.safe_mode(safe_mode),
            None => builder,
        }
        .build()
        .unwrap()
    }

    fn info<'a>(kernel: &'a Kernel, id: &str) -> &'a paperpaw_sdk::PluginInfo {
        kernel.plugins().get(id).unwrap()
    }

    #[test]
    fn code_is_loaded_in_the_order_of_ids_with_its_folder() {
        let loader = loader();
        let kernel = kernel(&loader, "", None);
        assert_eq!(
            *loader.asked.borrow(),
            ["plugins/hello/code.ppn", "plugins/zeta/code.ppn"]
        );
        assert!(*loader.finished.borrow());
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.state, hello.kind, hello.problem),
            (PluginState::Running, PluginKind::Code, None)
        );
        assert_eq!(info(&kernel, "theme").kind, PluginKind::Data);
        assert!(kernel.shared.actions.contains("hello.hello"), "its code's");
        assert_eq!(
            kernel.translator().get("hello.greeting"),
            Some("Hi"),
            "its folder's"
        );
        let native = kernel.shared.native.as_ref().unwrap();
        assert_eq!(native.safe_mode, None);
    }

    #[test]
    fn switched_off_its_code_is_not_loaded() {
        let loader = loader();
        let kernel = kernel(&loader, "[plugins]\nhello = off\n", None);
        assert_eq!(*loader.asked.borrow(), ["plugins/zeta/code.ppn"]);
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.state, hello.enabled, hello.kind),
            (PluginState::Off, false, PluginKind::Code)
        );
    }

    #[test]
    fn code_that_does_not_fit_is_listed_with_why() {
        let loader = Loader {
            answers: alloc::vec![
                ("plugins/hello/code.ppn", Err(PluginProblem::OtherBuild)),
                ("plugins/zeta/code.ppn", Ok("other")),
            ],
            ..Loader::default()
        };
        let kernel = kernel(&loader, "", None);
        let hello = info(&kernel, "hello");
        assert_eq!(hello.state, PluginState::Failed);
        assert!(hello.enabled, "it stays switched on, for a build that fits");
        assert_eq!(hello.problem, Some(PluginProblem::OtherBuild));
        assert_eq!(info(&kernel, "zeta").problem, Some(PluginProblem::Mismatch));
        assert!(!kernel.shared.actions.contains("other.hello"));

        // Where PaperPaw loads no code.
        let kernel = Kernel::builder(DEVICE).storage(storage()).build().unwrap();
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.state, hello.problem),
            (PluginState::Failed, Some(PluginProblem::Unsupported))
        );
        assert!(kernel.shared.native.is_none());
    }

    #[test]
    fn safe_mode_switches_off_the_plugin_the_crash_was_in() {
        let loader = loader();
        let mut safe_mode = SafeMode::new(SafeModeReason::FailedStarts);
        safe_mode.crashed = Some(String::from("zeta"));
        let kernel = kernel(&loader, "", Some(safe_mode));
        assert!(loader.asked.borrow().is_empty(), "no code at all");
        let zeta = info(&kernel, "zeta");
        assert_eq!(
            (zeta.enabled, zeta.problem),
            (false, Some(PluginProblem::Crashed))
        );
        // Not loaded at this start, but on for the next.
        let hello = info(&kernel, "hello");
        assert_eq!((hello.state, hello.enabled), (PluginState::Off, true));
        assert_eq!(hello.problem, Some(PluginProblem::SafeMode));
        assert!(hello.restart_needed());
        let config = paperpaw_sdk::read_file(kernel.storage(), "paperpaw.ini", 4096).unwrap();
        let config = String::from_utf8(config).unwrap();
        assert!(config.contains("zeta = off"), "{config}");
        assert!(!config.contains("hello"), "{config}");
        let native = kernel.shared.native.as_ref().unwrap();
        let safe = native.safe_mode.as_ref().unwrap();
        assert_eq!(safe.switched_off, [Id::new_static("zeta")]);
    }

    #[test]
    fn safe_mode_without_a_culprit_switches_every_code_plugin_off() {
        for reason in [SafeModeReason::FailedStarts, SafeModeReason::KeyHeld] {
            let loader = loader();
            let mut safe_mode = SafeMode::new(reason);
            // Held at power-on, a crash before is not what it is about.
            safe_mode.crashed = (reason == SafeModeReason::KeyHeld).then(|| String::from("zeta"));
            let kernel = kernel(&loader, "[plugins]\nzeta = off\n", Some(safe_mode));
            let native = kernel.shared.native.as_ref().unwrap();
            let safe = native.safe_mode.as_ref().unwrap();
            assert_eq!(
                safe.switched_off,
                [Id::new_static("hello")],
                "{reason:?}: zeta was off"
            );
            let hello = info(&kernel, "hello");
            assert_eq!(
                (hello.enabled, hello.problem),
                (false, Some(PluginProblem::SafeMode))
            );
        }
    }

    #[test]
    fn code_installed_while_running_starts_after_a_restart() {
        let loader = loader();
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new())
            .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
            .build()
            .unwrap();
        let _ = kernel.storage().create_dir("plugins/hello");
        for (path, data) in [
            ("plugins/hello/plugin.ini", "[plugin]\nname = Hello\n"),
            ("plugins/hello/code.ppn", "PPNP"),
        ] {
            kernel.storage().write(path, data.as_bytes()).unwrap();
        }
        kernel.load_plugin("plugins/hello");
        assert!(loader.asked.borrow().is_empty());
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.state, hello.enabled, hello.kind),
            (PluginState::Off, true, PluginKind::Code)
        );
        assert!(hello.restart_needed());
    }
}
