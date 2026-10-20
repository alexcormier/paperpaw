//! Wi-Fi, where PaperPaw manages it itself (the X4): when the radio is on,
//! which network it joins, and the page to choose networks (see
//! `paperpaw_sdk::wifi`). Another plugin may take its place: the shell
//! only does what it is told, and the networks joined are kept with the
//! passwords and keys, the same for any plugin.
//!
//! By default the radio is off and comes on only when a plugin wants the
//! network (a catalog, a sync, a transfer), joins the strongest network
//! joined before, and goes off again a minute after nothing uses it: it
//! costs battery and memory.

#![no_std]

extern crate alloc;

mod page;
mod policy;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::wifi::{self, JoinFailure, WifiCommand, WifiState};
use paperpaw_sdk::work::Work;
use paperpaw_sdk::{
    Arg, Command, Env, Id, Manifest, MenuNeed, Params, Plugin, RegisterError, Registrar,
    SDK_VERSION, ScreenSpec, SettingGroupSpec, SettingSpec, StatusItemSpec, StatusValue, Version,
};

use paperpaw_sdk::setting::{Setting, Text};
use policy::{Goal, Policy};

pub const PLUGIN: Id = Id::new_static("wifi");
/// The settings group, shown as the Wi-Fi page.
pub const GROUP: Id = Id::new_static("wifi.group");
/// The Wi-Fi page.
pub const PAGE: Id = Id::new_static("wifi.page");
/// The status item: the Wi-Fi sign while the device is in a network.
pub const STATUS: Id = Id::new_static("wifi.status");
/// The radio comes on by itself when a plugin wants the network.
pub const ON_DEMAND: Setting<bool> = Setting::new("wifi.on-demand");
/// How long after the last use the radio goes off.
pub const IDLE_OFF: Setting<Text> = Setting::new("wifi.idle-off");
/// The radio stays on (in the network joined before) while charging.
pub const WHILE_CHARGING: Setting<bool> = Setting::new("wifi.while-charging");
/// The keyboard's answer: a network's password.
pub(crate) const PASSWORD_ENTERED: Event<Typed> = Event::new("wifi.password-entered");
/// The timer to switch the radio off.
const IDLE_CHECK: Id = Id::new_static("wifi.idle-check");
/// The timer to try the networks joined before again.
const RETRY: Id = Id::new_static("wifi.retry");
/// How long after a round that joined nothing it is tried again.
const RETRY_MS: u64 = 10_000;
/// The parameter of [`IDLE_CHECK`]: the policy's generation it was set at.
const GENERATION: &str = "generation";

/// The choices of [`IDLE_OFF`] and their seconds.
pub(crate) const IDLE_OFF_CHOICES: [(&str, u64); 5] = [
    ("wifi.idle-off.30s", 30),
    ("wifi.idle-off.1m", 60),
    ("wifi.idle-off.2m", 120),
    ("wifi.idle-off.5m", 300),
    ("wifi.idle-off.10m", 600),
];

pub(crate) type Shared = Rc<RefCell<Policy>>;

/// The radio stays on now: charging, and the user wants that.
fn charging_keeps_it(env: &Env<'_>) -> bool {
    // Offline keeps the radio off also while charging.
    env.settings.read(&WHILE_CHARGING)
        && env.status.battery.is_some_and(|b| b.charging)
        && env.power.online()
}

fn idle_seconds(env: &Env<'_>) -> u64 {
    let choice = env.settings.read(&IDLE_OFF);
    IDLE_OFF_CHOICES
        .iter()
        .find(|(id, _)| *id == choice)
        .map_or(60, |(_, seconds)| *seconds)
}

fn wifi_commands(out: &mut Vec<Command>, commands: Vec<WifiCommand>) {
    out.extend(commands.into_iter().map(Command::Wifi));
}

/// Sets the timer to switch the radio off, if nothing keeps it on.
fn schedule_off(env: &Env<'_>, policy: &Policy, out: &mut Vec<Command>) {
    let Some(wifi) = env.wifi else {
        return;
    };
    if wifi.state.is_on() && policy.may_go_off(charging_keeps_it(env)) {
        let at = env.now.add_millis(idle_seconds(env) * 1000);
        let params = Params::new().with(GENERATION, alloc::format!("{}", policy.generation));
        out.push(Command::Work(Work::at(IDLE_CHECK, at).with(params)));
    }
}

/// After a round that joined nothing: sets the timer to try again, if
/// the network is still wanted (or charging keeps the radio on), or else
/// the one to switch the radio off.
fn schedule_retry(env: &Env<'_>, policy: &Policy, out: &mut Vec<Command>) {
    let Some(wifi) = env.wifi else {
        return;
    };
    if policy.still_wanted(wifi, env.now) || charging_keeps_it(env) {
        let params = Params::new().with(GENERATION, alloc::format!("{}", policy.generation));
        out.push(Command::Work(
            Work::at(RETRY, env.now.add_millis(RETRY_MS)).with(params),
        ));
    } else if !wifi.busy {
        schedule_off(env, policy, out);
    }
}

/// No network is coming for what waits: it fails now, and the network
/// counts as no longer wanted.
fn no_network(policy: &mut Policy, out: &mut Vec<Command>) {
    policy.asked_at = None;
    out.push(Command::NoNetwork);
}

/// Why joining `ssid` failed, in words.
fn failure_text(env: &Env<'_>, ssid: &str, reason: JoinFailure) -> String {
    let key = match reason {
        JoinFailure::Password => "wifi.failed.password",
        JoinFailure::NotFound => "wifi.failed.not-found",
        JoinFailure::Timeout => "wifi.failed.timeout",
        JoinFailure::Other => "wifi.failed.other",
    };
    env.i18n.format(key, &[("ssid", Arg::Str(ssid))])
}

/// The Wi-Fi plugin.
#[derive(Debug, Default)]
pub struct Wifi;

/// How long the radio stays on unused, to choose from.
const IDLE_OFF_IDS: [&str; IDLE_OFF_CHOICES.len()] = {
    let mut ids = [""; IDLE_OFF_CHOICES.len()];
    let mut i = 0;
    while i < IDLE_OFF_CHOICES.len() {
        ids[i] = IDLE_OFF_CHOICES[i].0;
        i += 1;
    }
    ids
};

/// When the radio is on.
static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::fixed_switch(
        ON_DEMAND.static_str(),
        "wifi.setting.on-demand",
        true,
        GROUP,
    )
    .described("wifi.setting.on-demand.help"),
    SettingSpec::fixed_choice(
        IDLE_OFF.static_str(),
        "wifi.setting.idle-off",
        &IDLE_OFF_IDS,
        "wifi.idle-off.1m",
        GROUP,
    )
    .described("wifi.setting.idle-off.help"),
    SettingSpec::fixed_switch(
        WHILE_CHARGING.static_str(),
        "wifi.setting.while-charging",
        false,
        GROUP,
    )
    .described("wifi.setting.while-charging.help"),
];

impl Plugin for Wifi {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Wi-Fi"),
            description: Cow::Borrowed(
                "Joins Wi-Fi networks when needed and switches the radio off after.",
            ),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
            uses: paperpaw_sdk::Uses::NETWORK,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let policy: Shared = Rc::new(RefCell::new(Policy::default()));

        // The settings, on the group's own page; only where PaperPaw
        // manages Wi-Fi.
        r.add_setting_group(SettingGroupSpec {
            id: GROUP,
            title: Cow::Borrowed("wifi.title"),
            order: 85,
            page: Some(PAGE),
        })?;
        r.add_icon(GROUP, Cow::Borrowed("wifi"))?;
        r.add_status_item(StatusItemSpec {
            id: STATUS,
            title: Cow::Borrowed("wifi.status.item"),
            value: Box::new(|env, _| {
                env.wifi
                    .is_some_and(|w| w.state.is_online())
                    .then_some(StatusValue::Icon(Cow::Borrowed("wifi")))
            }),
        })?;
        r.add_settings(&SETTINGS)?;
        for spec in &SETTINGS {
            r.set_menu_need(spec.id.clone(), MenuNeed::Wifi)?;
        }
        let p = policy.clone();
        r.add_screen(ScreenSpec {
            id: PAGE,
            title: Cow::Borrowed("wifi.title"),
            factory: Box::new(move |env, _| Box::new(page::WifiPage::new(env, p.clone()))),
        })?;

        // A plugin wants the network.
        let p = policy.clone();
        r.on(events::NETWORK_WANTED, move |env, _, out| {
            let Some(wifi) = env.wifi else {
                return;
            };
            if !env.settings.read(&ON_DEMAND) {
                return;
            }
            let mut policy = p.borrow_mut();
            policy.asked_at = Some(env.now);
            let known = wifi::known(env.secrets);
            if known.is_empty() {
                // Nothing to join: what waits says so now.
                out.push(Command::Notify(env.i18n.tr("wifi.no-saved").into()));
                no_network(&mut policy, out);
            }
            wifi_commands(out, policy.wanted(&wifi.state, &known));
        })?;

        // The radio's state changed.
        let p = policy.clone();
        r.on(events::WIFI_CHANGED, move |env, _, out| {
            let Some(wifi) = env.wifi else {
                return;
            };
            let mut policy = p.borrow_mut();
            if let WifiState::Online { ssid, .. } = &wifi.state
                && let Some((pending, password)) = policy.pending.take()
            {
                if pending == *ssid {
                    out.push(Command::SetSecret(wifi::keep(&pending, &password)));
                } else {
                    policy.pending = Some((pending, password));
                }
            }
            wifi_commands(out, policy.changed(&wifi.state));
            // On and in no network while charging keeps it on (e.g.
            // switched on again, or a network left): join one.
            if wifi.state == WifiState::On
                && policy.goal == Goal::Nothing
                && charging_keeps_it(env)
                && !wifi::known(env.secrets).is_empty()
            {
                wifi_commands(out, policy.retry());
            }
            // On, with nothing to do (e.g. joined, nothing waits).
            if !wifi.busy && !policy.still_wanted(wifi, env.now) {
                schedule_off(env, &policy, out);
            }
        })?;

        // Networks found.
        let p = policy.clone();
        r.on(events::WIFI_SCANNED, move |env, _, out| {
            let Some(wifi) = env.wifi else {
                return;
            };
            let known = wifi::known(env.secrets);
            let mut policy = p.borrow_mut();
            match policy.scanned(&wifi.networks, &known) {
                Some(commands) => wifi_commands(out, commands),
                None => {
                    policy.goal = Goal::Nothing;
                    if policy.still_wanted(wifi, env.now) {
                        out.push(Command::Notify(env.i18n.tr("wifi.none-in-reach").into()));
                        no_network(&mut policy, out);
                    }
                    schedule_retry(env, &policy, out);
                }
            }
        })?;

        // Joining a network failed.
        let p = policy.clone();
        r.on(events::WIFI_FAILED, move |env, failed, out| {
            let Some(wifi) = env.wifi else {
                return;
            };
            let (ssid, reason) = (failed.ssid.as_str(), failed.reason);
            let known = wifi::known(env.secrets);
            let mut policy = p.borrow_mut();
            let chosen = matches!(policy.goal, Goal::Chosen { .. });
            match policy.failed(ssid, &wifi.networks, &known) {
                Some(commands) if !commands.is_empty() => wifi_commands(out, commands),
                // The last try (or the user's choice): say why.
                result => {
                    if result.is_none() {
                        policy.goal = Goal::Nothing;
                        schedule_retry(env, &policy, out);
                    }
                    // Said when the user waits for it, not for tries
                    // in the background.
                    if chosen || policy.still_wanted(wifi, env.now) {
                        out.push(Command::Notify(failure_text(env, ssid, reason)));
                    }
                }
            }
        })?;

        // The password typed for a network.
        let p = policy.clone();
        r.on(PASSWORD_ENTERED, move |env, typed, out| {
            let (Some(wifi), Some(ssid)) = (env.wifi, typed.context.get(params::ID)) else {
                return;
            };
            let password = typed.text.as_str();
            let mut policy = p.borrow_mut();
            policy.pending = Some((String::from(ssid), String::from(password)));
            let commands = policy.choose(&wifi.state, ssid, password);
            wifi_commands(out, commands);
        })?;

        // Nothing uses the network: off a while later.
        let p = policy.clone();
        r.on(events::NETWORK_IDLE, move |env, _, out| {
            schedule_off(env, &p.borrow(), out)
        })?;
        // Another power mode: a radio only charging kept on may go off.
        let p = policy.clone();
        r.on(events::POWER_CHANGED, move |env, (), out| {
            if env.wifi.is_some_and(|w| !w.busy) {
                schedule_off(env, &p.borrow(), out);
            }
        })?;
        let p = policy.clone();
        r.subscribe(
            IDLE_CHECK,
            Box::new(move |env, params, out| {
                let Some(wifi) = env.wifi else {
                    return;
                };
                let policy = p.borrow();
                let current = params.get(GENERATION).and_then(|g| g.parse::<u32>().ok())
                    == Some(policy.generation);
                if current
                    && wifi.state.is_on()
                    && !wifi.busy
                    && !policy.still_wanted(wifi, env.now)
                    && policy.may_go_off(charging_keeps_it(env))
                {
                    out.push(Command::Wifi(WifiCommand::Off));
                }
            }),
        )?;

        // Another round, if the network is still wanted.
        let p = policy.clone();
        r.subscribe(
            RETRY,
            Box::new(move |env, params, out| {
                let Some(wifi) = env.wifi else {
                    return;
                };
                let mut policy = p.borrow_mut();
                let current = params.get(GENERATION).and_then(|g| g.parse::<u32>().ok())
                    == Some(policy.generation);
                if !current || wifi.state != WifiState::On || policy.goal != Goal::Nothing {
                    return;
                }
                if (policy.still_wanted(wifi, env.now) || charging_keeps_it(env))
                    && !wifi::known(env.secrets).is_empty()
                {
                    wifi_commands(out, policy.retry());
                } else if !wifi.busy {
                    // What waited has failed meanwhile.
                    schedule_off(env, &policy, out);
                }
            }),
        )?;

        // On while charging, if the user wants that.
        for event in [events::STARTED, events::STATUS_CHANGED] {
            let p = policy.clone();
            let started = event == events::STARTED;
            r.subscribe(
                event,
                Box::new(move |env, _, out| {
                    let Some(wifi) = env.wifi else {
                        return;
                    };
                    let mut policy = p.borrow_mut();
                    // The status changes with every percent of the
                    // battery: only a change of charging counts.
                    let keeps = charging_keeps_it(env);
                    if keeps == policy.charging_kept && !started {
                        return;
                    }
                    policy.charging_kept = keeps;
                    if keeps {
                        if !wifi.state.is_on() {
                            let known = wifi::known(env.secrets);
                            wifi_commands(out, policy.wanted(&wifi.state, &known));
                        }
                    } else if !wifi.busy && !policy.still_wanted(wifi, env.now) {
                        schedule_off(env, &policy, out);
                    }
                }),
            )?;
        }

        // The first-run setup: a network to join, where PaperPaw runs
        // the radio.
        r.add_setup_step(paperpaw_sdk::setup::SetupStepSpec {
            id: Id::new_static("wifi.setup"),
            title: Cow::Borrowed("wifi.title"),
            help: Cow::Borrowed("wifi.setup.help"),
            order: 50,
            page: paperpaw_sdk::setup::SetupPage::Screen(PAGE),
            applies: Some(Box::new(|env: &Env<'_>| env.wifi.is_some())),
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}
