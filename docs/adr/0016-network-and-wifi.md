# 0016. Network and Wi-Fi

- Status: Accepted
- Date: 2026-10-18

## Context

Plugins reach the network through HTTP requests, servers and sockets
([ADR 0014](0014-sockets-for-plugins.md)). On Android and desktops the
system manages Wi-Fi; QuireInk only uses the network. On the X4 (and
later on Linux readers) QuireInk has to manage the radio itself: switch
it on and off, find networks, join one with a password the user typed.

The radio is the largest consumer of battery after the display and,
on the X4, takes about 47 KB of a heap of a few hundred: it must be off
while reading and on only while something needs the network. When and
which network is a question of taste and of use (a catalog, a sync, a
transfer, a device on its charger), so it must be replaceable like
everything else ([ADR 0004](0004-everything-is-a-plugin.md)). The kernel
stays sans-I/O ([ADR 0003](0003-sans-io-kernel.md)).

## Decision

- **Mechanism in the shell, policy in a plugin.** The shell does what
  it is told and reports what happens; deciding when the radio is on
  and which network it joins, and the page to choose networks, are the
  `wifi` plugin's. Another plugin can take its place.
- **The vocabulary is in the SDK** (`quireink_sdk::wifi`):
  `WifiCommand` (on, off, scan, join, leave), `WifiState` (off, on,
  joining, online with the address), `WifiNetwork` and `JoinFailure`.
  Plugins send `Command::Wifi`; `Env::wifi` is `Some` only where
  QuireInk manages Wi-Fi, with the state, the networks found last,
  whether work waits for the network (`wanted`) and whether something
  uses it (`busy`). Events: `core.wifi-changed`, `core.wifi-scanned`,
  `core.wifi-failed`, `core.network-wanted`, `core.network-idle`.
- **The kernel** (built with `KernelBuilder::wifi`) passes the commands
  to the shell (`Io::Wifi`, `Kernel::take_io`) and takes its reports
  (`Kernel::wifi_event`). Network work of plugins (requests, servers,
  sockets) **waits in the kernel** while there is no network, for up to
  `NETWORK_WAIT_MS` (30 s), and goes to the shell once the state is
  online; then it fails as without a network. When work starts to wait
  the kernel emits `core.network-wanted`; when nothing uses the network
  any more, `core.network-idle`. `Env::network` keeps its meaning: the
  device can reach a network, not that it does now.
- **Known networks are shared data**: their passwords are kept with the
  passwords and keys as `wifi.<name>` (`quireink_sdk::wifi::{known,
  keep, forget}`), so a plugin taking the place of `wifi` finds them, and
  the user sees and removes them in the settings.
- **The `wifi` plugin's policy**: on demand (a setting) it switches the
  radio on when the network is wanted and joins the strongest known
  network, the next one on failure, another round a little later while
  still wanted; it switches the radio off a while after the network is
  idle (a setting), unless the user switched it on on its page or it
  stays on while charging (a setting; on by default on the X4, which
  charges from USB). Its page is the settings group's own page
  (`SettingGroupSpec::page`); its settings are shown only where QuireInk
  manages Wi-Fi (`MenuNeed::Wifi`).
- **Bodies stream on the shell's side**: a request's file to send
  (`HttpCall::body_from`) is read by the shell, and a body meant for a
  file (`HttpCall::save_to`) may be written there by the shell as it
  comes (`HttpResponse::saved`). The X4 reads and writes in parts, so
  neither ever needs to fit in memory; shells with memory to spare may
  load the file (`HttpCall::load_body`).
- **The X4 shell**: the radio (esp-radio) is started when told and
  dropped when off, which powers it down and frees its memory; scanning
  and joining run in the background, polled by the main loop. HTTP goes
  over smoltcp with the protocol in `quireink-http`, a crate without
  I/O. Servers and sockets are not there yet: they fail at once with a
  reason.
- **The emulator** pretends to be such a device for the X4 profiles:
  made-up networks and delays, the computer's network once "online", so
  the plugin and what waits for a network can be tried and tested
  without hardware.

## Consequences

- The radio is on only while needed, and a plugin can change when.
- Plugins need not care whether the network is there: their work waits
  for it a while, and the radio comes on for it.
- Kernel tests cover waiting, failing and the events without a network;
  the plugin's decisions are tested on their own; the emulator shows the
  page and the whole path.
- A new platform that manages Wi-Fi implements the five commands and
  reports the state; the rest comes along.
- Servers and sockets on the X4 and HTTPS ([ADR 0017](0017-tls-on-the-x4.md))
  build on this; updates over the air will too.
