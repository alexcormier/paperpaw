//! The remote control's commands (`tools/x4-remote.py`, parsed by
//! [`paperpaw_x4::remote::parse`]) on the browser's device: the same lines,
//! the same kernel calls as on an X4, answered as JSON instead of `PP:`
//! lines, so that a page (or an assistant driving it) opens screens,
//! sets settings and reads what is shown directly rather than by presses
//! and taps. What only a device has (its panel, radio, flash, sleep) is
//! refused.

use paperpaw_core::sdk::{Id, Params, PluginKind};
use paperpaw_dev::report::json_string;
use paperpaw_platform::PointerPhase;
use paperpaw_x4::remote::{self, Command, Touch};

use super::Emulator;

/// `{}`: done.
const DONE: &str = "{}";

impl Emulator {
    /// Carries out the remote control's command `line` at `now_ms` (as
    /// [`Emulator::advance`] counts), time passed to then first; its
    /// answer as JSON:
    ///
    /// - `run <action>`, `slot <slot> [<key>=<hex> …]`, `show <screen>
    ///   [<key>=<hex> …]`, `open <hex path>`, `set <setting> <hex value>`,
    ///   `emit <event> [<key>=<hex> …]`, `type <hex>`, `type-enter <hex>`
    ///   (an error where no text field shows), `press <button>`, `hold <button>
    ///   <ms>`, `touch <down|move|up> <x> <y>`, `time set <unix ms>`,
    ///   `rm <path>`, `mkdir <path>`, `mv <hex> <hex>`: `{}`;
    /// - `screen`: `{"screen": id}`; `screens`: `{"screens": [ids]}`;
    /// - `describe`: `{"screen": id, "parts": [{"role", "x", "y",
    ///   "width", "height", "state", "focused", "on", "disabled", "label"}]}`
    ///   (state as the remote's: `focused`, `on`, `off`, `disabled`, joined
    ///   by `+`, or `-`; the same as `focused` and `disabled` true or false
    ///   and `on` true, false or null);
    /// - `setting <id>`: `{"value": text}`; `settings [<prefix>]`:
    ///   `{"values": {id: text}}`; `actions [<prefix>]`: `{"actions":
    ///   [ids]}`; `plugins`: `{"plugins": [{"id", "on", "version",
    ///   "kind"}]}`; `version`: `{"version", "device"}`; `time`:
    ///   `{"unix_ms", "ms"}`; `usage`: `{"refreshes": [fast, partial,
    ///   full], "pixels", "inputs"}`; `ls [<path>]`: `{"entries": [{"name",
    ///   "dir", "size"}]}`; `stat <path>`: `{"kind": "f"|"d"|"-",
    ///   "size"}`.
    ///
    /// Secrets are never said (`•`), as on an X4.
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    pub fn command(&mut self, line: &str, now_ms: u64) -> Result<String, String> {
        let command = remote::parse(line).ok_or_else(|| format!("no command: {line}"))?;
        self.advance(now_ms);
        // Presses and touches as the page's own (their time counted).
        match command {
            Command::Press { button, ms } => {
                let name = button;
                if !self.button(name, true, now_ms) {
                    return Err(format!("no button `{name}`"));
                }
                self.button(name, false, now_ms + ms);
                return Ok(String::from(DONE));
            }
            Command::Touch { phase, x, y } => {
                let phase = match phase {
                    Touch::Down => PointerPhase::Down,
                    Touch::Move => PointerPhase::Move,
                    Touch::Up => PointerPhase::Up,
                };
                self.pointer(phase, x, y, now_ms);
                return Ok(String::from(DONE));
            }
            _ => {}
        }
        let device = self.device.as_mut().ok_or("the device has not started")?;
        let id = |text: &str| Id::parse(text).map_err(|_| format!("no id: {text}"));
        let params = |pairs: Vec<(String, String)>| {
            let mut params = Params::new();
            for (key, value) in pairs {
                params.set(key, value);
            }
            params
        };
        Ok(match command {
            Command::Run(action) => {
                let action = id(&action)?;
                device.call(|kernel, now| kernel.run_action(now, action));
                String::from(DONE)
            }
            Command::Slot {
                id: slot,
                params: pairs,
            } => {
                let (slot, params) = (id(&slot)?, params(pairs));
                device.call(|kernel, now| kernel.open_slot(now, slot, params));
                String::from(DONE)
            }
            Command::Show {
                id: screen,
                params: pairs,
            } => {
                let (screen, params) = (id(&screen)?, params(pairs));
                device.call(|kernel, now| kernel.open_screen(now, screen, params));
                String::from(DONE)
            }
            Command::Open(path) => {
                device.call(|kernel, now| kernel.open_book(now, &path));
                String::from(DONE)
            }
            Command::Emit {
                event,
                params: pairs,
            } => {
                let (event, params) = (id(&event)?, params(pairs));
                device.call(|kernel, now| kernel.send_event(now, event, params));
                String::from(DONE)
            }
            Command::Type { text, done } => {
                if !device.type_text(&text, done) {
                    return Err(String::from("no text field"));
                }
                String::from(DONE)
            }
            Command::Set { id: setting, value } => {
                let mut refused = None;
                device.call(
                    |kernel, now| match kernel.set_setting(now, &setting, &value) {
                        Ok(effects) => effects,
                        Err(why) => {
                            refused = Some(why.to_string());
                            kernel.draw_again(now)
                        }
                    },
                );
                if let Some(why) = refused {
                    return Err(why);
                }
                String::from(DONE)
            }
            Command::SetTime(unix_ms) => {
                device.set_time(unix_ms);
                String::from(DONE)
            }
            Command::Screen => format!(
                "{{\"screen\":{}}}",
                json_string(device.kernel.active_screen().map_or("", |s| s.as_str()))
            ),
            Command::Screens => {
                let ids: Vec<String> = device
                    .kernel
                    .screens()
                    .map(|s| json_string(s.as_str()))
                    .collect();
                format!("{{\"screens\":[{}]}}", ids.join(","))
            }
            Command::Describe => describe(&mut device.kernel),
            Command::Setting(setting) => {
                let text = said(device.kernel.settings(), &setting)
                    .ok_or_else(|| format!("no setting `{setting}`"))?;
                format!("{{\"value\":{}}}", json_string(&text))
            }
            Command::Settings(prefix) => {
                let settings = device.kernel.settings();
                let values: Vec<String> = settings
                    .specs()
                    .filter(|spec| spec.id.as_str().starts_with(prefix.as_str()))
                    .filter_map(|spec| {
                        let text = said(settings, spec.id.as_str())?;
                        Some(format!(
                            "{}:{}",
                            json_string(spec.id.as_str()),
                            json_string(&text)
                        ))
                    })
                    .collect();
                format!("{{\"values\":{{{}}}}}", values.join(","))
            }
            Command::Actions(prefix) => {
                let ids: Vec<String> = device
                    .kernel
                    .actions()
                    .iter()
                    .filter(|a| a.id.as_str().starts_with(prefix.as_str()))
                    .map(|a| json_string(a.id.as_str()))
                    .collect();
                format!("{{\"actions\":[{}]}}", ids.join(","))
            }
            Command::Plugins => {
                let plugins: Vec<String> = device
                    .kernel
                    .plugins()
                    .iter()
                    .map(|p| {
                        let kind = match p.kind {
                            PluginKind::BuiltIn => "built-in",
                            PluginKind::Data => "data",
                            PluginKind::Code => "code",
                            _ => "other",
                        };
                        format!(
                            "{{\"id\":{},\"on\":{},\"version\":{},\"kind\":\"{kind}\"}}",
                            json_string(p.manifest.id.as_str()),
                            p.enabled,
                            json_string(&p.manifest.version.to_string())
                        )
                    })
                    .collect();
                format!("{{\"plugins\":[{}]}}", plugins.join(","))
            }
            Command::Version => format!(
                "{{\"version\":{},\"device\":{}}}",
                json_string(&super::version()),
                json_string(device.kernel.device().id)
            ),
            Command::Time => {
                format!(
                    "{{\"unix_ms\":{},\"ms\":{}}}",
                    device
                        .unix_millis()
                        .map_or(String::from("null"), |ms| ms.to_string()),
                    device.now().millis()
                )
            }
            Command::Usage => {
                let u = device.kernel.usage();
                let [fast, partial, full] = u.refreshes;
                format!(
                    "{{\"refreshes\":[{fast},{partial},{full}],\"pixels\":{},\"inputs\":{}}}",
                    u.pixels, u.inputs
                )
            }
            Command::List(path) => {
                let mut entries = Vec::new();
                device
                    .kernel
                    .storage()
                    .list(&path, &mut |entry| {
                        entries.push(format!(
                            "{{\"name\":{},\"dir\":{},\"size\":{}}}",
                            json_string(entry.name),
                            entry.is_dir,
                            entry.size
                        ));
                    })
                    .map_err(|e| format!("{path}: {e:?}"))?;
                format!("{{\"entries\":[{}]}}", entries.join(","))
            }
            Command::Stat(path) => {
                let storage = device.kernel.storage();
                let (kind, size) = if let Ok(size) = storage.size(&path) {
                    ("f", size)
                } else if storage.list(&path, &mut |_| {}).is_ok() {
                    ("d", 0)
                } else {
                    ("-", 0)
                };
                format!("{{\"kind\":\"{kind}\",\"size\":{size}}}")
            }
            Command::Remove(path) => {
                let storage = device.kernel.storage();
                let removed = if storage.size(&path).is_ok() {
                    storage.remove(&path)
                } else {
                    storage.remove_dir_all(&path)
                };
                removed.map_err(|e| format!("{path}: {e:?}"))?;
                String::from(DONE)
            }
            Command::MakeDir(path) => {
                device
                    .kernel
                    .storage()
                    .create_dir(&path)
                    .map_err(|e| format!("{path}: {e:?}"))?;
                String::from(DONE)
            }
            Command::Move { from, to } => {
                device
                    .kernel
                    .storage()
                    .rename(&from, &to)
                    .map_err(|e| format!("{from}: {e:?}"))?;
                String::from(DONE)
            }
            _ => return Err(format!("not in the browser: {line}")),
        })
    }

    /// Whether the screen shown is `text` (its id) or shows it (in the
    /// label of a part, as the remote's `until:` and `expect:` look).
    pub fn shows(&mut self, text: &str) -> bool {
        let Some(device) = self.device.as_mut() else {
            return false;
        };
        if device
            .kernel
            .active_screen()
            .is_some_and(|s| s.as_str() == text)
        {
            return true;
        }
        !device.kernel.is_secret()
            && device
                .kernel
                .describe()
                .iter()
                .any(|p| p.label.contains(text))
    }
}

/// A setting's value as the remote says it: as configuration files write
/// it, a secret never (`•`); `None` for no such setting.
fn said(settings: &paperpaw_core::sdk::Settings, id: &str) -> Option<String> {
    let value = settings.get(id)?;
    let secret = settings.spec(id).is_some_and(|spec| spec.extra().secret);
    Some(if secret {
        String::from("•")
    } else {
        value.to_config_text()
    })
}

/// What the screen shows, part by part (as the remote's `describe`).
fn describe(kernel: &mut paperpaw_core::Kernel) -> String {
    // A PIN or a password being entered: where the parts are, not what
    // they say.
    let secret = kernel.is_secret();
    let screen = kernel
        .active_screen()
        .map(|s| s.as_str().to_string())
        .unwrap_or_default();
    let parts: Vec<String> = kernel
        .describe()
        .into_iter()
        .map(|part| {
            let state = remote::part_state(part.focused, part.on, part.disabled);
            let label = if secret { String::from("•") } else { part.label.replace('\n', " ") };
            let a = part.area;
            let on = part.on.map_or(String::from("null"), |on| on.to_string());
            format!(
                "{{\"role\":\"{:?}\",\"x\":{},\"y\":{},\"width\":{},\"height\":{},\"state\":{},\"focused\":{},\"on\":{on},\"disabled\":{},\"label\":{}}}",
                part.role,
                a.x,
                a.y,
                a.width,
                a.height,
                json_string(&state),
                part.focused,
                part.disabled,
                json_string(&label)
            )
        })
        .collect();
    format!(
        "{{\"screen\":{},\"parts\":[{}]}}",
        json_string(&screen),
        parts.join(",")
    )
}
