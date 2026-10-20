# 0025. Power saving: power levels, modes and request priorities

- Status: Proposed
- Date: 2026-10-20

## Context

Battery is precious, most of all on the X4 (650 mAh) and the X4 Pro
(1,100 mAh). Plugins sync with services (KOReader positions, statistics,
Readwise, Joplin, OPDS catalogs), widgets show weather and news, rules
run actions at times and battery levels, and the radio is switched on
for all of this by the `wifi` plugin ([ADR 0016](0016-network-and-wifi.md)).
Readers want to say "make the battery last" once, and have every part of
the system, including plugins written by others, take less.

### Where the energy goes today

Read from the code on 2026-10-20 (a survey of the shells, the kernel and
every first-party plugin). Numbers marked *measured* are in the
CHANGELOG or the ADRs; numbers marked *derived* are arithmetic from the
chips' data sheets and have not been measured on a device. The battery
plugin's logs exist for exactly this, but no analysis of them has been
written down yet.

Two facts frame everything. On battery, sleeping on the X4 is switching
off (D2): nothing runs while the device sleeps, and every wake is a full
start. So only awake time costs energy, and a mode can only save while
the device is in use or idle before it sleeps. And the ESP32's states
differ by a factor of a hundred: light sleep about 0.1 mA (*measured*),
the CPU awake at full speed about 20 mA on the X4 (*measured*), the
radio receiving about 85 mA and sending up to 300 mA (data sheet).

1. **The radio, while on.** The shell never sets the radio's power
   saving (esp-radio defaults to none, so it listens all the time), and
   the main loop does not light-sleep while the radio is on
   (`shells/xteink/src/firmware.rs`, `battery = !self.network.radio_on()
   && …`), so it busy-waits in 10 ms steps. After the last request the
   radio stays on for `wifi.idle-off` (1 min by default). A connection
   scans twice (the plugin's scan, then the join's), waits for SNTP on
   the X4 (no clock chip), and takes 1.3 to 2.7 s for TLS (*measured*,
   ADR 0017). One small background request therefore costs roughly
   70 s at around 100 mA, about 2 mAh (*derived*): on the order of one
   to two hours of reading with light sleep.
2. **kosync turns the radio on by itself.** It is on by default and,
   once signed in, pulls the position when a book opens and on every
   wake, and pushes it when a book closes and before sleep, without
   asking whether the radio is on anyway (`plugins/kosync/src/lib.rs`).
   It is the largest background user of the network by default. (The
   push before sleep probably never leaves the X4: the kernel holds the
   request until online, and the firmware switches off first.)
3. **The X4 Pro's front light** blocks light sleep (its PWM stops in a
   light sleep), so with the light above 0 % the ESP32-S3 spins at
   240 MHz the whole time, on top of the LEDs' current. Where the light
   is used, this is likely the largest avoidable cost on the Pro.
4. **Display refreshes.** A fast refresh takes about 0.5 s, the X4's
   half refresh against ghosting 1.6 s, a full one 3.5 s (*measured*).
   Every 6th partial refresh becomes a half refresh
   (`core.full-refresh-interval`). The panel's power is never switched
   off after a fast refresh (`power_off` is always `false`). While the
   panel refreshes, the CPU spins on its BUSY line. An idle home screen
   redraws its status line every minute without asking for a mode, so
   the kernel takes `Partial`, which counts towards the half refresh:
   one partial a minute and a half refresh every six minutes until the
   device sleeps (read from the code, not yet seen on a device).
5. **The CPU while awake.** The clock is fixed at the maximum
   (`CpuClock::max()`): 160 MHz on the X4, 240 MHz on the X4 Pro. On
   battery it light-sleeps at most 30 ms at a time (the buttons are
   read on each wake), so it wakes about 33 times a second. Counting a
   book's pages after opening runs in 2 ms steps, too short to sleep in.
6. **Every wake is a start**: the SD card mounted, the book reopened, a
   half refresh, kosync's pull, SNTP on the X4.
7. **Small**: the SD card written 2 s after each change, the battery
   plugin's line a minute, timers (cheap on their own: the loop wakes
   every 30 ms anyway; they cost only where they redraw or reach the
   network).

Several of these are not choices a reader should have to make: they are
costs that Normal should not have either (see *Before the modes*).

### How others do it

- **Android Battery Saver** restricts background work and background
  network, may turn on the dark theme, and is switched from a quick
  settings tile, by battery level, or by a routine; it turns off when
  charging. Apps read `PowerManager.isPowerSaveMode()` and get
  `ACTION_POWER_SAVE_MODE_CHANGED`. **Doze** goes further: no network,
  and background jobs, syncs and alarms of all apps run together in
  rare *maintenance windows*. **WorkManager** lets work state
  constraints (battery not low, unmetered, charging) and a *flex*
  window, so the system can run many jobs on one wake.
- **iOS Low Power Mode** pauses background refresh, mail fetch and
  downloads, dims the display, shortens auto-lock and slows the CPU;
  the battery icon turns yellow; it offers itself at 20 % and switches
  off at 80 % while charging. Apps read `isLowPowerModeEnabled` and get
  a notification. `URLSession` transfers can be *discretionary*: the
  system sends them when it suits.
- **Kindle and Kobo** quote battery life "with wireless off"; their
  levers are airplane mode, sleep and power-off timers, the front
  light and fewer full refreshes. Boox has a power save switch that
  freezes background apps, and switches Wi-Fi off on sleep. KOReader
  asks or turns Wi-Fi on when an action needs it and off when done.
- **What batching gains**: setting up the link (scan, join, DHCP, TLS)
  costs far more than the data. A study of phones found Wi-Fi scan and
  join cost five times a 50 KB transfer, and batching with deadlines
  saved 35 to 52 % for mail and news.

What an e-reader on an ESP32 gains most from, in order: sleeping
between interactions (a factor of a hundred), the radio off unless
needed and never idling while connected (a factor of ten to a hundred),
fewer link setups by batching, the front light, then the CPU clock and
refreshes (percent).

## Decision

### Before the modes: what Normal should not cost

These are fixed in the shells and plugins regardless of any mode, as
separate work items, because a mode should trade features for battery,
not hide costs nobody chose:

- the radio's power saving on while connected (modem sleep), and light
  sleep with the radio on where esp-radio allows it;
- light sleep (or a timed nap of the waveform's length) while the panel
  refreshes, instead of spinning on BUSY;
- the panel's power off after a fast refresh once nothing follows for a
  moment (checked to look the same);
- the X4 Pro's front light kept running through light sleep (LEDC on a
  clock that keeps running; to be tried on the S3);
- the home screen's minute redraw asking for `Fast`, so it never counts
  towards the half refresh;
- one scan per connection (the join reuses the plugin's scan or the
  channel of the last network joined);
- measuring all of it with the battery plugin first, so every later
  change has a baseline.

### Power levels: what plugins understand

`paperpaw_sdk::power` adds `PowerLevel`, three steps every plugin can
read and act on:

- **Normal**: everything as configured.
- **Saving**: keep every feature, but on demand. No network of one's
  own that turns the radio on (send it as background work, below);
  intervals of one's own at least four times as long; nothing redrawn
  on a timer that can wait for the next page turn or tap; no gray
  refresh that is not needed for a picture.
- **Offline**: reading only. No network of one's own at all; the radio
  stays off; what the user asks for that needs the network asks first
  ("Offline mode: go online for this?").

`Env::power` gives the level and the mode in use (`Power { level, mode,
automatic }`); `core.power-changed` (`events::POWER_CHANGED`) is emitted
when either changes. A helper turns an interval into the level's
(`power::every(env, base) -> Option<Duration>`: `base` in Normal, four
times `base` in Saving, `None` in Offline), so that most plugins adapt
with one line. The levels are fixed and few on purpose: a plugin author
can handle three cases, not every mode a reader invents.

### Modes: what readers choose

A **mode** is a level plus a set of setting values, worn while the mode
is in use. Four are built in, kept simple:

| Mode | Level | Values it brings (defaults, each editable) |
|------|-------|--------------------------------------------|
| **Normal** | Normal | none |
| **Saving** | Saving | `wifi.idle-off` 30 s, `core.sleep-after` 5 min, the front light at most 30 %, `core.full-refresh-interval` 10 |
| **Offline** | Offline | `wifi.on-demand` off, `core.sleep-after` 5 min, the front light at most 20 % |
| **Automatic** | chosen | switches between the three by the battery (below) |

**Automatic** is not a set of values but a chooser: Saving below
`power.saving-below` (30 %), Offline below `power.offline-below` (10 %),
Normal again above each level plus 5 % (so it does not flap), and
Normal while charging (`power.normal-while-charging`, on). Where the
system has its own saver (Android's Battery Saver), Automatic also
follows it: Saving while the system saves. When Automatic changes the
mode it says so once ("Saving mode: battery below 30 %").

**Mode values are a layer, not a change.** Wearing a mode puts its
values over the reader's own (a layer in `Settings`, read before the
stored value and the book's scope, after a preview); leaving it takes
the layer away, and the reader's own values are there as before.
Nothing is written. A number may be given as a limit instead of a
value (`x4.frontlight <= 30`): the reader's own value is kept below it.
If the reader changes a setting the mode holds
while it is worn, their change wins: the value is kept as theirs and
the mode lets go of that setting until the mode changes.

**Anyone can add modes.** A mode is a preset of the kind
`core.power-mode` ([presets](../../crates/paperpaw-sdk/src/presets.rs)),
with its level as one of its values (`power.level = saving`): built in
(`Registrar::add_presets`, costing next to no memory), brought by a
plugin, or by a data plugin's `presets.ini`:

```ini
[commute]
kind = core.power-mode
name = Commute
description = Offline, a dim light, sleep after 3 minutes.
power.level = offline
core.sleep-after = 3
x4.frontlight <= 10
```

Readers make their own the way they make profiles: *Save as new mode*
takes the level of the mode in use and the values they choose.

### Modes and profiles: one building block, two uses

Reading profiles (C6, the `profiles` plugin) and modes are both named
sets of setting values, and both can be made by readers and brought by
plugins. They differ in what they are for:

- a **profile** is how reading looks ("Large print", "Night"): it is
  *applied*, its values written into the reader's settings, and stays
  until changed by hand;
- a **mode** is how much energy to spend: it is *worn*, its values a
  layer taken away again, it carries a level that plugins act on, and
  Automatic changes it on its own.

They stay two concepts for the reader, on two axes that combine freely
(Night in Saving, Large print in Offline). One concept would let
Automatic change a reader's font at 30 %, or make a look carry a level
nobody meant. They share their building blocks: presets as the format
(two kinds, `core.power-mode` and, for profiles later, `profiles.profile`),
the same *save as*, rename and remove on the device, and both reachable
from rules and buttons. A profile may name a mode to wear with it
(`power.mode = core.offline`), so "Night" can mean "dim, warm and
offline" with one tap.

### Network requests have a priority

`HttpRequest` (and `Command::Connect`) get `priority: NetworkPriority`:

- **`User`**: someone waits for it (a catalog opened, *Sync now*). It
  turns the radio on now, in every level; in Offline after the
  question.
- **`Soon`**: work of the plugin's own the reader expects to happen,
  but does not watch (kosync's pull when a book opens). Turns the
  radio on in Normal; in Saving and Offline it waits like `Background`.
- **`Background`**: may happen whenever (statistics, weather, news,
  catalogs synced by a rule). Never turns the radio on by itself. It
  waits in the kernel until the radio is on for anything else, until
  the device charges, or until its deadline (`HttpRequest::by`,
  e.g. 6 h); at the deadline, in Normal, the radio is switched on once
  for everything waiting. In Saving it waits for one of the first two;
  in Offline it is not sent and fails at once, so the plugin keeps it.

Requests without a priority keep working: one sent while the kernel
handles the reader's input counts as `User`, any other as `Soon`. So
plugins written before this behave sensibly, and those that mark their
background work save more.

The kernel keeps waiting background requests in a short queue (at most
8; a body to send stays on the card, `body_from`), sends them all
together when the radio comes on, and answers them with `status 0` and
`deferred` when the device goes to sleep, so the plugin keeps the work
for next time (the queue does not survive switching off). The
`core.network-wanted` event says whether the reader waits
(`params::USER`), so the `wifi` plugin decides, by level:

| | Normal | Saving | Offline |
|-|--------|--------|---------|
| `User` | radio on | radio on | asks, then radio on |
| `Soon` | radio on | waits | waits |
| `Background` | waits (deadline: radio on) | waits | refused |
| radio off after idle | `wifi.idle-off` | 30 s | at once |
| while charging | `wifi.while-charging` | as Normal | radio off |

Servers and sockets (Wi-Fi transfer, Calibre) are always opened by the
reader and count as `User`. A plugin may also ask whether sending now is
cheap (`power::radio_on(env)`), as kosync and weather do by hand today.

### The shells

The kernel tells the shell the level (`Io::Power(PowerLevel)`); what a
shell does with it is its own. A shell says what it can do with it
(`KernelBuilder::power`, read as `Env::capabilities.power`: which of
radio, CPU clock, front light and system saver it manages), so that the
settings show only what applies. The X4 shell, in Saving and Offline:

- the CPU at 80 MHz outside drawing, decoding and TLS (back to the
  maximum for those: finishing fast and sleeping is cheaper);
- light sleep up to about 60 ms between button reads instead of 30 (a
  press lasts at least 80 ms), and on the X4 Pro waking on its buttons'
  and touch controller's interrupts instead;
- the card written back after 10 s instead of 2;
- on start, no network unless the reader asks (no kosync pull on wake).

Android reports the system's Battery Saver as the shell's saver and
otherwise leaves the system to manage Wi-Fi and the CPU; the levels'
meaning for plugins is the same there. The emulator offers the levels
and a battery that can be set, so all of this can be tried without
hardware.

### First-party plugins

| Plugin | Saving | Offline |
|--------|--------|---------|
| `wifi` | the table above; one connection for everything waiting | radio off, a question for `User` work |
| `kosync` | pull and push only while the radio is on anyway (`Background`); *Sync progress* stays | nothing by itself; *Sync progress* asks |
| `weather` | interval at least 4 h (`power::every`), only while online anyway (as today) | the last forecast, marked as old |
| `news` | already on demand; the widget keeps the last headlines | the last headlines |
| `stats`, `opds`, `update`, `calibre`, `wallabag` | on demand already; syncs started by rules sent as `Background` | rules' syncs not sent |
| `readwise`, `joplin` | exports wait for the radio (they already retry when it comes) | kept for later |
| `home`, `widgets` | no minute wake: the clock and live widgets update with the next tap or button; clocks in whole minutes | as Saving |
| `reader` | the status line's clock updates with the page turn; page counting deferred until idle or charging; smooth text (grays) off | as Saving |
| `sleep` | the cover drawn without grays | a message, no picture |
| `rules` | unchanged: rules are the reader's own; triggers *power mode changed*, effect *set power mode* | unchanged |
| `battery` | keeps measuring; its log gets the mode, radio time and requests per plugin | as Saving |

### How a reader sees and switches it

- **Quick panel**: *Power* as four segments (Normal, Saving, Offline,
  Automatic) with buttons and touch alike; a reader's own modes open a
  list from there.
- **Status item** `core.status.power`: a small leaf next to the
  battery, shown only when not in Normal (the yellow battery of iOS),
  placed on the home screen's and the reader's lines by default; drawn
  empty in Normal so the line stays calm.
- **Settings › System › Power**: the mode, Automatic's two levels and
  *Normal while charging*, the list of modes with what each changes,
  *Save as new mode*. The setting `core.power-mode` (`auto` or a mode's
  id) is an ordinary choice, so rules, buttons and the menu editor reach
  it like any other.
- **A question** where Offline holds something back, and a notice when
  Automatic switches; nothing else interrupts.

### Where it lives

- **SDK**: `power` (`PowerLevel`, `Power`, `NetworkPriority`, `every`,
  `radio_on`), `Env::power`, `events::POWER_CHANGED`, the settings'
  mode layer, `HttpRequest::priority` and `by`. Additive; `HttpRequest`
  gains fields, so plugins that build it field by field instead of with
  `HttpRequest::new` need a change: a minor SDK version
  ([ADR 0020](0020-sdk-compatibility.md)), noted in the CHANGELOG.
- **Kernel**: wears the mode (the layer), emits the event, tells the
  shell, keeps and sends the background queue.
- **A new `power` plugin**, replaceable like `wifi`: the built-in
  modes, Automatic, the page in the settings, the quick panel entry,
  the status item, the notices and questions. Mechanism in the kernel
  and the shells, policy in a plugin, as in ADR 0016.

### Defaults for beginners

Automatic, with Saving below 30 % and Offline below 10 %, Normal while
charging. A beginner never has to find the setting: the device starts
saving when it matters, says so once, shows a leaf while it does, and
goes back by itself when charged. Nothing they use stops working in
Saving; in Offline, what needs the network asks.

### Expected gain

Rough, *derived* from data sheets and the survey, to be measured with
the battery plugin before and after:

- **The fixes before the modes** likely gain most: the radio idling
  with modem sleep instead of listening, and light sleep during
  refreshes, take a background request from about 2 mAh to well under
  half of that; on the X4 Pro with the light on, keeping light sleep
  could cut the awake current by an order of magnitude.
- **Saving** removes nearly all background radio use: for a reader
  whose kosync pulls on every book open and wake, several of those a
  day are several mAh a day, a noticeable share of 650 mAh. The CPU at
  80 MHz saves about a quarter of the awake current outside drawing;
  the longer button nap halves the wakes. The front light cap saves in
  proportion. A reader who never uses Wi-Fi gains little from Saving
  beyond the light and the sleep timer: their device already saves.
- **Offline** adds no radio at all and the shortest timers: it is for
  the last 10 %, so that the book can be finished.

## Consequences

- One switch, chosen by hand, by battery or by a rule, makes every
  part of the system and every plugin that reads the level take less;
  plugins that do not read it still save through the request priority
  and the shell.
- Plugins get a contract (three levels, a helper for intervals, a
  priority on requests) instead of each inventing `radio_on_anyway`.
- Background network is batched: the radio's setup is paid once for
  everything waiting.
- The reader's own settings are never overwritten by a mode; profiles
  and modes stay separate but share their format and editing.
- The kernel holds a little more state (the layer, a queue of at most
  8 requests); the modes themselves are presets kept in the program.
- Work waits that used to happen at once; plugins must say what the
  reader waits for, or the fallback guesses (input handled now: `User`).
- Gains are claimed only after the battery plugin has measured them.

## Open questions

1. **Modes and profiles**: two concepts sharing presets (proposed), or
   one list of "modes" each of which may or may not carry a level and
   may be applied or worn? One is fewer words for beginners, two keeps
   Automatic from touching looks.
2. **Automatic's defaults**: 30 % and 10 %? Or Saving at the existing
   *battery low* level (20 %, `core.battery-low`) and no automatic
   Offline, only an offer at 10 %?
3. **Offline and the reader's own actions**: ask each time (proposed),
   ask once per wake, or refuse quietly with a notice? Should Offline
   also hold back Wi-Fi transfer and Calibre?
4. **What each built-in mode brings**: the values in the table above,
   in particular the sleep timer (a shorter one means more starts, and
   every start on the X4 costs a half refresh and a mount) and the
   front light caps.
5. **Should Saving change looks** (e.g. no gray covers on the sleep
   screen, no smooth text), or only energy that the reader does not
   see?
6. **The fallback priority** for unmarked requests (`User` while input
   is handled, `Soon` otherwise): good enough, or should unmarked
   requests count as `User` always, and only marked ones be deferred?
7. **Where the quick panel shows custom modes**: four segments fit
   exactly; with more, a list, or segments for the built-in ones and
   *More…*?
8. **Charging**: Normal while charging in Automatic (proposed), or keep
   saving until a level (iOS: 80 %) so that a short charge does not undo
   it?
9. **Timers with slack** (a window instead of a time, as Android's
   flex), so the kernel can wake once for several plugins: worth it on
   the X4, where timers are cheap and the radio is the cost, or later?
