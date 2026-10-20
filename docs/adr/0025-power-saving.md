# 0025. Power saving: efficient by default, declared work, power modes

- Status: Proposed
- Date: 2026-10-20

## Context

Battery is precious, most of all on the X4 (650 mAh) and the X4 Pro
(1,100 mAh). Plugins sync with services (KOReader positions, statistics,
Readwise, Joplin, OPDS catalogs), widgets show weather and news, rules
run actions at times and on events, and the radio is switched on for all
of this by the `wifi` plugin ([ADR 0016](0016-network-and-wifi.md)).

PaperPaw should be very frugal **by default**, without getting in the
reader's way: a position read on the X4 must be on the phone when the
reader picks it up, not an hour later. Saving should come from doing
the same work at better moments and with less waste, not from doing
less of what the reader wants. Where readers want to trade features for
battery, one switch should do it, for every plugin, including those
written by others.

### Where the energy goes today

Read from the code on 2026-10-20 (a survey of the shells, the kernel and
every first-party plugin). Numbers marked *measured* are in the
CHANGELOG or the ADRs; numbers marked *derived* are arithmetic from the
chips' data sheets, not measured on a device. The battery plugin logs
what is needed, but no analysis of its logs has been written down yet.

Two facts frame everything. On battery, sleeping on the X4 is switching
off (D2): nothing runs while the device sleeps, and every wake is a full
start. So only awake time costs energy, and work can only happen while
the device is awake. And the ESP32's states differ by a factor of a
hundred: light sleep about 0.1 mA (*measured*), the CPU awake at full
speed about 20 mA on the X4 (*measured*), the radio receiving about
85 mA and sending up to 300 mA (data sheet).

1. **The radio, while on.** The shell never sets the radio's power
   saving (esp-radio defaults to none, so it listens all the time), and
   the main loop does not light-sleep while the radio is on
   (`shells/xteink/src/firmware.rs`, `battery = !self.network.radio_on()
   && …`); it busy-waits in 10 ms steps. After the last request the
   radio stays on for `wifi.idle-off` (1 min by default). A connection
   scans twice (the plugin's scan, then the join's), waits for SNTP on
   the X4 (no clock chip), and takes 1.3 to 2.7 s for TLS (*measured*,
   ADR 0017). One small background request costs roughly 70 s at around
   100 mA, about 2 mAh (*derived*): on the order of one to two hours of
   reading with light sleep.
2. **Each plugin decides on its own when to reach the network.**
   kosync, on by default, pulls the position when a book opens and on
   every wake and pushes it when a book closes and before sleep, each
   time switching the radio on by itself (`plugins/kosync/src/lib.rs`).
   Weather and kosync's page-turn push check by hand whether the radio
   is on anyway (`radio_on_anyway`, `online()`); the others do not. The
   push before sleep probably never leaves the X4: the kernel holds it
   until online, and the firmware switches off first.
3. **The X4 Pro's front light** blocks light sleep (its PWM stops in a
   light sleep), so with the light above 0 % the ESP32-S3 spins at
   240 MHz the whole time, on top of the LEDs' current.
4. **Display refreshes.** A fast refresh takes about 0.5 s, the X4's
   half refresh against ghosting 1.6 s, a full one 3.5 s (*measured*).
   The panel's power is never switched off after a fast refresh
   (`power_off` is always `false`). While the panel refreshes, the CPU
   spins on its BUSY line. An idle home screen redraws its status line
   every minute without asking for a mode; the kernel takes `Partial`,
   which counts towards the periodic half refresh: one refresh a minute
   and a half refresh every six minutes until the device sleeps (read
   from the code, not yet seen on a device).
5. **The CPU while awake.** The clock is fixed at the maximum
   (`CpuClock::max()`): 160 MHz on the X4, 240 MHz on the X4 Pro. On
   battery it light-sleeps at most 30 ms at a time (the buttons are read
   on each wake): about 33 wakes a second. Counting a book's pages after
   opening runs in 2 ms steps, too short to sleep in.
6. **Every wake is a start**: the SD card mounted, the book reopened, a
   half refresh, kosync's pull, SNTP on the X4.
7. **Small**: the SD card written 2 s after each change, the battery
   plugin's line a minute, timers on their own.

### How others do it

- **Android** lets apps declare background work instead of running it
  (WorkManager, JobScheduler): constraints (network, charging, battery
  not low), a *flex* window instead of a time, unique names that replace
  each other. The system runs many apps' work on one wake, and in
  **Doze** only in rare *maintenance windows*. **Battery Saver** defers
  background work further, may turn on the dark theme, is switched from
  a quick settings tile, by battery level or by a routine, and turns off
  while charging. Apps read `isPowerSaveMode()` and get a broadcast when
  it changes.
- **iOS Low Power Mode** pauses background refresh, mail fetch and
  downloads, dims the display and slows the CPU; the battery icon turns
  yellow; it offers itself at 20 % and switches off at 80 % while
  charging. Background tasks (`BGTaskScheduler`) give only an earliest
  time; *discretionary* transfers are sent when it suits the system.
- **Kindle and Kobo** quote battery life "with wireless off"; their
  levers are airplane mode, sleep and power-off timers, the front light
  and fewer full refreshes. KOReader turns Wi-Fi on when an action
  needs it (or asks) and off when done.
- **Batching**: setting up the link (scan, join, DHCP, TLS) costs far
  more than the data. A study of phones found Wi-Fi scan and join cost
  five times a 50 KB transfer; batching with deadlines saved 35 to 52 %
  for mail and news.

What an e-reader on an ESP32 gains most from, in order: sleeping between
interactions (a factor of a hundred), the radio off unless needed and
never idling while connected (a factor of ten to a hundred), fewer link
setups by doing all network work together at the right moments, the
front light, then the CPU clock and refreshes (percent).

## Decision

Three layers, each building on the one before:

1. **Efficient by default**: the shells, the kernel and the first-party
   plugins waste nothing in any mode.
2. **Declared work**: plugins say what they need done and by when; the
   kernel decides when, and does it together with everything else, at
   moments that cost little. This replaces timers of plugins
   (`Command::Schedule`) and is the only way background work reaches the
   network.
3. **Power modes** change only the limits of declared work and a few
   settings (deadlines longer, optional work off, the light capped), by
   hand, by battery level or by a rule.

### 1. Efficient by default

Part of every mode, each change measured with the battery plugin before
and after (the plugin first gains what it lacks for this: radio time,
connections and requests per plugin, and the mode in each line):

- **The radio**: modem power saving while connected; light sleep with
  the radio on where esp-radio allows it; one scan per connection (the
  join uses the plugin's scan, or the channel of the network joined
  last); off at once after background work that no reader watched, and
  after `wifi.idle-off` (1 min) only after the reader's own use, when
  the next page of a catalog may follow.
- **The panel**: light sleep (or a timed nap of the waveform's length)
  while it refreshes, instead of spinning on BUSY; its power switched
  off after a fast refresh once nothing follows for a moment (checked to
  look the same).
- **The front light (X4 Pro)**: kept lit through light sleep (LEDC on a
  clock that keeps running in light sleep; to be tried on the S3), so
  reading with the light costs the LEDs and nothing more.
- **No idle redraws**: a screen redraws on a timer only what visibly
  changed, always as `Fast`, never counting towards the half refresh.
  The home screen's status line and clocks follow this.
- **The CPU**: page counting and other work that may wait run as
  declared work when the device is idle (below), in steps long enough
  to sleep between.
- **Network at natural moments**: first-party plugins declare their
  work (below), so that a reading session costs one short connection
  when it starts and one when it ends, whatever is installed.

### 2. Declared work

`paperpaw_sdk::work` adds `Work`: something a plugin wants done, with
what it needs and by when. When it is time, the kernel emits the work's
event with its parameters (as `Command::Schedule` did), and the plugin
does it then.

```rust
// Progress: pushed at the next natural moment, at the latest in 30 min.
out.push(Command::Work(
    Work::network(PUSH)            // needs the network
        .expected()                // the reader counts on it
        .within(Duration::from_secs(30 * 60))
        .at(Moments::BOOK_CLOSED | Moments::SLEEP | Moments::ONLINE),
));
// Weather: whenever the radio is on anyway, at most once an hour.
out.push(Command::Work(
    Work::network(FETCH).optional().every(Duration::from_secs(3600)),
));
// The rules' "every day at 22:00": a time, exactly (was Command::Schedule).
out.push(Command::Work(Work::at(TIME, at_22).with(params)));
```

What a work says:

- **its id** (an event of the plugin): declaring the same id again
  replaces it, so a plugin need not track what it declared;
  `Command::CancelWork(id)` takes it back;
- **what it needs**: nothing, the network, or the device charging
  (`charging()`, e.g. a large download of covers);
- **how much the reader counts on it**: `expected()` (a position, an
  export the reader sent: it will be done, at the latest by its
  deadline) or `optional()` (weather, news, a catalog refreshed: done
  when it is cheap, left out when it is not);
- **when**: not before (`after`), at the latest (`within`; none means
  only when it is cheap, never on its own account), or exactly (`at`, a
  time: what `Command::Schedule` was); and `every`, to declare it again
  after each run;
- **at which moments it may run**: `Moments`, see below; all of them
  when not said.

**The kernel runs work** whose *after* has passed:

- at a **natural moment** it may run at: `WAKE` (the device started or
  woke), `BOOK_OPENED`, `BOOK_CLOSED`, `SLEEP` (going to sleep),
  `ONLINE` (the radio is on anyway, for the reader or for other work),
  `CHARGING` (plugged in) and `IDLE` (no input for a while, the screen
  still on);
- when its **deadline** comes: then, if it needs the network, it is the
  one thing that may switch the radio on by itself, and it takes along
  everything else that needs the network and whose *after* has passed;
- at its **time**, for work with an exact time.

So the network is set up once for everything due: one connection on
waking (kosync's pull, the weather, the news, an export waiting), one on
going to sleep (the push, the statistics), and none in between unless a
deadline comes. **Going to sleep** waits for the work running then, for
at most `SLEEP_WORK_MS` (about 15 s; the sleep screen is already shown,
so the reader does not wait), before switching off. Work that needs the
network and was not done is written down and is due at the next wake
(the X4 does not know how long it slept).

**A run** lasts as long as the requests made while handling the work's
event, and the requests made while handling their answers, are not
answered; the kernel knows them, so a plugin need not say when it is
done. While a run needs the network, the radio stays on; once no run
and no reader needs it, the radio goes off. A failed request is the
plugin's to declare again (with a later *after*, the kernel offers a
backoff: `Work::retry(attempt)`).

**What the reader asks for is not work.** A request made while the
kernel handles the reader's input (a catalog opened, *Sync now*) goes
out at once and switches the radio on, as today. A request outside
both (an older plugin, an action run by a rule) is treated as expected
work due at once with a deadline of `NETWORK_WAIT_MS` (30 s): the same
as today in Normal, and limited by the mode like any other work. So
plugins written before this keep working, and the priority of a request
is not a separate concept: it is whether the reader waits (input), or
what the work declares.

**One model, not two.** `Command::Schedule` becomes `Work::at`: a time,
needing nothing, run exactly. The kernel keeps one queue of work (small:
an id, parameters and a few times each; at most 8 per plugin) and wakes
for the earliest of its times and deadlines. Screen timers
(`Cx::wake_at`) stay what they are: the open screen's own frame clock,
gone with the screen. The **rules** (C5) declare their times as exact
work, and their triggers are the same moments the kernel knows (a book
opened or closed, waking, going to sleep, charging), with one more,
*Wi-Fi came on*. An action a rule runs that reaches the network is
work like any other: "at 7:00: sync the catalogs" syncs at 7:00 if the
radio is on or the deadline allows, in Saving at the next connection.

**The `wifi` plugin** keeps deciding the radio ([ADR 0016](0016-network-and-wifi.md)):
`core.network-wanted` now says whether a reader waits or which work is
due, and the plugin switches on for either (in Offline, it asks first
when a reader waits, see below). It no longer needs retry rounds for
work that can wait: the kernel declares it due again.

### 3. Power levels and modes

`paperpaw_sdk::power` adds `PowerLevel`, three steps plugins may read
for what declared work does not cover (a widget's look, a reader's
page counting): **Normal**, **Saving**, **Offline**. `Env::power` gives
the level and the mode in use (`Power { level, mode, automatic }`);
`core.power-changed` is emitted when either changes. Most plugins never
need it: declaring work with the right limits is enough.

What a level does to declared work, in the kernel:

| | Normal | Saving | Offline |
|-|--------|--------|---------|
| expected work, deadline | as declared | four times as long | held, none runs on its own |
| optional work | when cheap | only at `ONLINE` and `CHARGING` | not run |
| requests the reader makes | at once | at once | a question first ("Offline mode: go online for this?") |
| radio after background work | off at once | off at once | stays off |
| work held back | | | runs at the first connection the reader allows, or when the mode changes |

These are settings (`core.work.stretch`, `core.work.optional`,
`core.work.network`), so a mode is a set of setting values, nothing
more.

**A mode** is a level plus a set of setting values, worn while the mode
is in use. Four are built in:

| Mode | Level | Values it brings (each editable) |
|------|-------|----------------------------------|
| **Normal** | Normal | none |
| **Saving** | Saving | the work limits above; the front light at most 50 %; `core.sleep-after` 5 min; the CPU at 80 MHz outside drawing, decoding and TLS; the buttons read every 60 ms instead of 30 |
| **Offline** | Offline | the work limits above; the front light at most 30 %; `core.sleep-after` 5 min; the CPU as in Saving |
| **Automatic** | chosen | Saving below `power.saving-below` (20 %), Offline below `power.offline-below` (10 %), Normal again 5 % above each, and Normal while charging |

**Mode values are a layer, not a change.** Wearing a mode puts its
values over the reader's own (a layer in `Settings`, read after a
preview, before the book's scope and the stored value); leaving it
takes the layer away, and the reader's own values are there as before.
Nothing is written. A number may be a limit instead of a value
(`x4.frontlight <= 50`): the reader's own value is kept below it. If
the reader changes a setting the mode holds, their change wins until
the mode changes.

**Anyone can add modes.** A mode is a preset of the kind
`core.power-mode` ([presets](../../crates/paperpaw-sdk/src/presets.rs))
with its level among its values: built in (`Registrar::add_presets`,
costing next to no memory), brought by a plugin, or by a data plugin's
`presets.ini`:

```ini
[commute]
kind = core.power-mode
name = Commute
description = Offline, a dim light, sleep after 3 minutes.
power.level = offline
core.sleep-after = 3
x4.frontlight <= 10
```

Readers make their own on the device: *Save as new mode* takes the
level of the mode in use and the values they choose.

**Modes and reading profiles** (C6, the `profiles` plugin) are one
building block with two uses. A profile is how reading looks ("Large
print", "Night"): it is *applied*, its values written into the reader's
settings, and stays until changed. A mode is how much energy to spend:
it is *worn*, a layer taken away again, and Automatic changes it on its
own. For the reader they stay two things on two axes that combine
freely (Night in Saving); one concept would let Automatic change a
reader's font at 20 %. They share the format (presets), the editing on
the device (*save as*, rename, remove) and the ways to reach them
(rules, buttons, the menu editor). A profile may name a mode to wear
with it (`power.mode = core.offline`), so "Night" can mean "dim, warm
and offline" with one tap.

### The shells

The kernel tells the shell the level and the shell's own values of the
mode (`Io::Power`); a shell says what it can change
(`KernelBuilder::power`, read as `Env::capabilities.power`: radio, CPU
clock, button reading, front light, a system saver), so that the
settings show only what applies there. The X4 shell applies the CPU
clock and the button interval; everything in *Efficient by default*
applies in every mode. Android follows the system's Battery Saver as
the shell's saver (Automatic is Saving while it is on), runs declared
work while PaperPaw is in front, and leaves Wi-Fi and the CPU to the
system. The emulator offers the levels and a battery that can be set,
so all of it can be tried without hardware.

### First-party plugins

| Plugin | Declares | Normal | Saving | Offline |
|--------|----------|--------|--------|---------|
| `kosync` | pull: expected, at `WAKE`, `BOOK_OPENED`, within 10 s; push: expected, at `BOOK_CLOSED`, `SLEEP`, `ONLINE`, within 30 min | position fresh on opening, on the other device as soon as the X4 is put away | the same moments; deadlines 40 s and 2 h | held; *Sync progress* asks |
| `weather` | fetch: optional, every `weather.every` (1 h) | when the radio is on anyway (as today) | at `ONLINE` and `CHARGING` | the last forecast, marked as old |
| `news` | refresh: optional, every few hours | at the next connection | at `ONLINE` and `CHARGING` | the last headlines |
| `stats` | sync: expected after reading, at `SLEEP`, `ONLINE`, within a day | at the next sleep | the same, within four days | held |
| `readwise`, `joplin` | an export: expected, within 30 min | at the next moment | within 2 h | held until online |
| `opds`, `update` | syncs and checks started by rules: expected, within an hour | at the next connection | within four hours | held |
| `calibre`, `transfer`, `browser`, `store` | nothing: the reader is there | at once | at once | a question first |
| `reader` | page counting: needs nothing, at `IDLE`, `CHARGING` | when idle | when idle | when idle |
| `home`, `widgets` | nothing; redraw only what changed | | clocks in whole minutes | as Saving |
| `wifi` | nothing; follows `core.network-wanted` | | | asks when a reader waits |
| `rules` | its times as exact work | | | |
| `battery` | measures; its log gains the mode, radio time, connections and requests per plugin | | | |

### How a reader sees and switches it

- **Quick panel**: *Power* as four segments (Normal, Saving, Offline,
  Automatic), the same by buttons and by touch.
- **Status item** `core.status.power`: a small leaf next to the battery,
  shown only when not in Normal (as the yellow battery on iOS), placed
  on the home screen's and the reader's lines by default, drawn empty in
  Normal so the lines stay calm.
- **Settings › System › Power**: the mode, Automatic's two levels and
  *Normal while charging*, the list of modes with what each changes,
  the reader's own modes and *Save as new mode*. The setting
  `core.power-mode` (`auto` or a mode's id) is an ordinary choice, so
  rules, buttons and the menu editor reach it like any other.
- **A notice** when Automatic switches ("Saving: battery below 20 %"),
  and in Offline a question where the reader asks for the network.
  Nothing else interrupts.

### Where it lives

- **SDK**: `work` (`Work`, `Moments`, `Command::Work`,
  `Command::CancelWork`), `power` (`PowerLevel`, `Power`), `Env::power`,
  `events::POWER_CHANGED`, the settings' mode layer and limits.
  `Command::Schedule` is removed in favour of `Work::at` (its callers
  are the rules, weather and wifi plugins). A minor SDK version
  ([ADR 0020](0020-sdk-compatibility.md)), noted in the CHANGELOG and
  the SDK's API file.
- **Kernel**: the queue of work, the moments, runs and their requests,
  the levels' limits, the mode layer, holding sleep for running work,
  writing down what was not done.
- **A new `power` plugin**, replaceable like `wifi`: the built-in modes,
  Automatic, the settings page, the quick panel entry, the status item
  and the notices. Mechanism in the kernel and the shells, policy in a
  plugin, as in ADR 0016.

### Defaults for beginners

Efficient by default, and Automatic: Saving below 20 % (the level at
which PaperPaw already calls the battery low), Offline below 10 %,
Normal while charging. A beginner never has to find the setting: the
device saves anyway, saves more when the battery runs low, says so
once, shows a leaf while it does, and goes back by itself when charged.
Nothing they use stops working in Saving; in Offline, what needs the
network asks.

### Expected gain

Rough, *derived* from the data sheets and the survey, to be measured
with the battery plugin before and after each step:

- **A reading session** today costs a connection for kosync's pull on
  waking, another for its push on closing a book, each with a 1 min
  tail without modem sleep or light sleep: about 2 mAh each, several a
  day, a noticeable share of 650 mAh. With declared work and the radio
  fixed: two short connections a session (waking, sleeping), each
  carrying everything due, a few tenths of a mAh each. The position is
  on the other device as soon as the X4 sleeps.
- **The X4 Pro with the light on**: keeping light sleep could cut the
  awake current by an order of magnitude.
- **Refreshes and the CPU**: no idle redraws, no spinning during
  refreshes and the panel's power off save a little on every page;
  Saving's slower clock and button reading about a quarter of the awake
  current outside drawing.
- **Saving and Offline** then add the last part: no optional network,
  longer deadlines, the light capped, no radio at all at the end of the
  battery, so the book can be finished.

## Consequences

- PaperPaw is frugal without a mode; the modes trade features for
  battery, and nothing a reader chose is overwritten by them.
- Plugins say what they need and by when, and get the network at the
  cheapest moment; none needs `radio_on_anyway` or its own timers and
  retries. Background work of all plugins shares one connection.
- One queue in the kernel for timers and work instead of a timer list
  and per-plugin policies; rules use the same model.
- Progress sync is on time where it matters: when a book is opened and
  when the X4 is put away.
- Going to sleep may take up to about 15 s longer, unseen behind the
  sleep screen; work not done is due at the next wake.
- `Command::Schedule` goes; its callers move to `Work::at`. Plugins
  that build `HttpRequest` field by field are not affected.
- Gains are claimed only after the battery plugin has measured them.

## Open questions, with a recommendation each

Marked *measure* where a measurement on the device should come first.

1. **Modes and profiles: one concept or two?** Recommended: two for
   the reader, one building block (presets, the same editing), and a
   profile may name a mode.
2. **Automatic's levels.** Recommended: Saving below 20 % (the existing
   *battery low* level, one number to know), Offline below 10 %, Normal
   5 % above each and while charging.
3. **Offline and what the reader asks for.** Recommended: ask each time
   ("Go online for this?" with *Go online* and *Not now*), also for
   Wi-Fi transfer and Calibre; work held back runs on that connection
   too.
4. **What the built-in modes bring.** Recommended: the values in the
   table above. *Measure*: the sleep timer (a shorter one means more
   starts, and every start on the X4 costs a half refresh, a mount and
   the wake's connection) and the light caps (what 50 % and 30 % save).
5. **Should Saving change looks?** Recommended: no, apart from the
   light cap and clocks in whole minutes: no grays dropped, no covers
   removed; their cost is small once refreshes sleep.
6. **Requests outside input and outside declared work.** Recommended:
   expected work due at once with the old 30 s deadline, limited by the
   mode; nothing to change for plugins written before this.
7. **Custom modes in the quick panel.** Recommended: the four built-in
   modes as segments; the reader's own modes in the list under
   Settings › System › Power and as actions for buttons and rules; a
   fifth row *More modes* only when there are some.
8. **Charging.** Recommended: Normal while charging (the X4 charges
   from USB; charging is also the moment optional work runs), and
   Automatic picks up again when unplugged.
9. **Timers with slack.** Recommended: yes, as declared work replacing
   `Command::Schedule` (decided above), with exact times for the rules.
10. **How long going to sleep may wait for work.** Recommended: about
    15 s, the radio off at once afterwards. *Measure*: how long a
    connection and a kosync push take on the X4 once the radio is fixed
    (the firmware logs `wifi: join took`).
11. **kosync's deadlines.** Recommended: pull within 10 s of opening
    (the reader expects the latest position, and it is the wake's one
    connection anyway), push at closing or sleeping and at the latest
    after 30 min of reading. *Measure*: a session's cost with these
    deadlines, against today.
12. **The base fixes that need the hardware.** Recommended: all of
    them, each kept only if it looks the same and measures better.
    *Measure*: modem sleep and light sleep with the radio on (what
    esp-radio allows), the panel's power off after fast refreshes, the
    front light through light sleep on the S3.
