# 0027. Bluetooth Low Energy

- Status: Accepted
- Date: 2026-10-24

## Context

Readers ask for Bluetooth page-turners: a small remote, or a keyboard,
that turns pages while the device rests on a stand. Plugin authors want
more: heart-rate belts and other sensors for reading statistics, a
phone companion that finds the reader without Wi-Fi, a foot pedal, a
smart light. A spike on the X4 Pro (branch `ble-spike`, bc7aea04) found:

- **It runs.** esp-radio's BLE controller with `coex` beside Wi-Fi, up
  in 4 ms, scanning 53 ms after the command; Wi-Fi joined during a scan.
  esp-radio gives an HCI connector (`BleConnector`) that implements
  bt-hci's `Transport`, so trouble-host (Embassy's host stack: central
  and peripheral, GATT client and server, LE Secure Connections) plugs
  in as it is.
- **It costs** about 75 KB of flash, 21.6 KB of IRAM for the
  controller's code, and 35.4 KB of internal RAM while the controller
  is up, all of it back when it is down.
- **A trap**: on the ESP32-S3, IRAM and DRAM are one SRAM, so the
  controller's code shrank the main stack from 43.4 KB to 19.8 KB and the
  firmware hit its stack guard at start. The spike guessed a smaller
  internal heap (160 KB instead of 192 KB). A product needs a budget
  that a build checks.
- **Battery is not measured.** Receiving costs some 80 to 95 mA at full
  duty, and the main loop does not light-sleep while a radio is on. So
  Bluetooth, like Wi-Fi ([ADR 0016](0016-network-and-wifi.md)), must be
  off unless something needs it, and its work must fit declared work
  ([ADR 0025](0025-power-saving.md)).

The forces are those of Wi-Fi and sockets
([ADR 0014](0014-sockets-for-plugins.md)): the kernel is sans-I/O
([ADR 0003](0003-sans-io-kernel.md)), the radio is shared by plugins
that must not see each other's devices or keep it on for nothing,
everything must be replaceable ([ADR 0004](0004-everything-is-a-plugin.md)),
and code plugins are bound to an interface version
([ADR 0026](0026-plugin-interface-version.md)).

## Decision

Bluetooth Low Energy (not Classic) becomes a capability of the kernel,
in the manner of Wi-Fi: **the mechanism in the shells, arbitration and
safety in the kernel, policy in a first-party plugin**, and a generic
vocabulary in the SDK that is not about page-turners. Page-turners come
for free on top: a bonded keyboard's keys are inputs of the key maps.

### 1. The SDK: `paperpaw_sdk::bluetooth`

A new module. Every type that may grow is `#[non_exhaustive]` with a
constructor (rule 1), errors are typed (rule 2), and builders take
`impl Into` (rule 7).

- **Who may**: `Uses::BLUETOOTH` in the manifest, shown in the store
  and the plugin manager like `Uses::NETWORK`. The kernel refuses every
  Bluetooth command of a plugin without it (`BtError::NotAllowed`).
- **Where**: `Capabilities::bluetooth` (the device has a radio the
  kernel may use), `MenuNeed::Bluetooth` for settings and menu entries.
  `Env::bluetooth()` is `Some` only there, with what every plugin may
  know: the radio's state (`Off`, `On`), whether something uses it, the
  most links the device keeps (`max_links`), and the bonded devices'
  names and kinds (no addresses).
- **Values**:
  - `Uuid`: 128 bits, with `Uuid::sig(0x180D)` for the Bluetooth SIG's
    16-bit numbers and `Uuid::parse`.
  - `DeviceId`: an opaque number the kernel gives a device. It is
    stable across starts for a bonded device; plugins never see
    addresses, which may be random and change.
  - `Device`: id, name, signal (`rssi`), the services it advertises,
    `bonded`, `DeviceKind` (`Keyboard`, `Remote`, `Sensor`, `Phone`,
    `Other`, from its appearance and services).
  - `Service`, `Characteristic` (uuid, a `Handle`, `Properties`: read,
    write, write without response, notify, indicate).
  - Values are bytes (`Vec<u8>`), carried in events as typed values
    (`Params::carrying`), as `HttpReply`'s body is.
- **Commands**: `Command::Bluetooth(Box<BtCommand>)`, written with
  builders:

  ```rust
  bluetooth::scan(&FOUND)            // results come as FOUND events
      .service(Uuid::sig(0x180D))    // filters: services, name prefix
      .for_ms(10_000)                // a scan always ends
      .command()
  bluetooth::connect(device, &LINK)  // all about the link: LINK events
  bluetooth::discover(device)        // services and characteristics
  bluetooth::read(device, handle)
  bluetooth::write(device, handle, bytes).without_response()
  bluetooth::subscribe(device, handle)    // notifications or indications
  bluetooth::unsubscribe(device, handle)
  bluetooth::pair(device)            // asks the user, see 3.
  bluetooth::disconnect(device)
  bluetooth::advertise(&SEEN)        // be seen, see below
      .name("PaperPaw")
      .service(MY_SERVICE)
      .for_ms(60_000)
      .command()
  bluetooth::notify(handle, bytes)   // to the centrals subscribed
  bluetooth::respond(request, bytes) // to a central's read
  ```

- **Events**: one event of the plugin's per request or link
  (`scan(&FOUND)`, `connect(device, &LINK)`), as sockets have, each a
  `BtEvent`: `Found(Device)`, `ScanEnded`, `Connected { device }`,
  `Disconnected { device, reason }`, `Discovered { device, services }`,
  `Value { device, handle, bytes }` (a read or a notification),
  `Written { device, handle }`, `Paired { device }`, `Failed(BtError)`. `BtError`: `Off`, `NotAllowed`, `NotFound`,
  `Busy` (no link left), `Timeout`, `Refused` (by the user), `Att(code)`
  (the device's GATT error), `Unsupported`. Each command a plugin sends
  is answered, as declared work is: nothing is refused silently.
- **A GATT server**: `Registrar::add_gatt_service(GattServiceSpec)`,
  declared at registration with its characteristics and their
  properties, so the kernel builds one attribute table for all plugins.
  A central's reads, writes and subscriptions come as the plugin's
  events (`BtEvent::ReadRequest`, `WriteRequest`, `Subscribed`); it
  answers with `respond` and sends with `notify`. Advertising is asked
  for with a duration and ends with it or with the screen that asked,
  whichever is first; the kernel merges what several plugins ask into
  one advertisement.
- **Background work**: `Work::bluetooth()`, beside `Work::network()`,
  for work that needs the radio (reading a sensor every hour), done at
  cheap moments like the network's ([ADR 0025](0025-power-saving.md)).

### 2. Input: keyboards and page-turners

- A bonded device that offers the HID service, and that the user let
  type when pairing (the default for keyboards and remotes), is the
  kernel's: it subscribes to its reports and turns them into button
  events, `InputEvent::Button { name: "hid.right", .. }` with press and
  release, so the key maps, long presses and rules see them like the
  device's buttons. No plugin parses HID for that.
- The names are a fixed table of the keyboard and consumer usages that
  matter (`hid.right`, `hid.left`, `hid.up`, `hid.down`, `hid.page-down`,
  `hid.page-up`, `hid.space`, `hid.enter`, `hid.escape`, `hid.volume-up`,
  `hid.volume-down`, `hid.play-pause`, `hid.next-track`,
  `hid.previous-track`, the letters and digits), in
  `paperpaw_platform::buttons`. The default key map turns pages with
  right, page down, space, volume up and next track (and back with their
  opposites), which covers the cheap remotes (most are HID keyboards
  sending arrows, page keys or volume). The keys plugin lists them under
  a Bluetooth heading where a remote is bonded.
- Decoding is a crate without I/O (`paperpaw-hid`): the boot keyboard
  report and consumer control reports by the device's report map,
  tested on recorded reports. The shell hands raw reports to the kernel,
  which decodes, so every shell gets the same keys.
- A plugin that wants the raw reports (a game pad, a pedal with its own
  report) asks with `bluetooth::hid_reports(device, &REPORTS)` for a
  device granted to it (3.); the user decides on the device's page
  whether its keys still go to the key maps.

### 3. Permissions and safety

- **Pairing is the user's.** A plugin may ask (`pair`), but the kernel
  asks the user in its own prompt (a core screen, not the plugin's):
  the device's name, and the code to compare or type where the device
  wants one (numeric comparison, passkey display or entry). Just Works
  pairing still needs a confirmation. A refusal is `BtError::Refused`.
- **A plugin sees only its devices**: those its own scans found, and
  bonded devices the user gave it (on the device's page in the
  Bluetooth settings, "Used by"; asked once when a plugin pairs one).
  Commands about other devices fail with `NotFound`, as if they were not
  there. Scans of several plugins are one scan for the radio, each
  plugin getting only the results that pass its filters.
- **Bonds are secrets.** The keys a bond makes (LTK, IRK) are kept with
  the passwords and keys as `bluetooth.<device>`, under `.paperpaw` like
  them, and the user sees and removes them there or on the Bluetooth
  page. The kernel hands them to the shell at start and takes new ones
  from it. Forgetting a device removes its keys and its grants.
- **Links end**: a plugin's scans, advertising and links end with the
  screen that was showing when it asked (as sockets do), unless they
  were asked for by declared work, which has its own limits; all end
  before the device sleeps. The kernel's HID links follow the policy
  plugin (5.).

### 4. The kernel and the shells

- **The kernel** (built with `KernelBuilder::bluetooth`) keeps each
  plugin's scans, links, subscriptions and advertising, numbers them for
  the shell (`Io::Bluetooth`, `Kernel::take_io`) and turns the shell's
  reports (`Kernel::bluetooth_event`) into the plugins' events. It
  switches the radio on when the first request comes and off when none
  is left (after a short grace for the next one), limits links to
  `max_links`, and emits `core.bluetooth-changed`. Kernel tests cover
  all of it with made-up shell reports.
- **The X4 Pro** (feature `bluetooth` of the firmware, later on by
  default): trouble-host on esp-radio's `BleConnector`, polled by the
  main loop as the radio's futures are. Its memory is a budget, not a
  guess:
  - static: the controller's IRAM (21.6 KB measured) taken from the
    internal heap at build time, with a check in the firmware's build
    (from the linker map) that the main stack keeps its size, so the
    spike's boot loop cannot come back unseen;
  - while up: the controller's 35.4 KB of internal RAM (measured), and
    the host's pools sized for two links (a remote and one more), an ATT
    MTU of 247 and the discovered services, put in PSRAM where the host
    allows; the total is measured and written down before the feature
    is on by default;
  - with Wi-Fi on at the same time: allowed, measured with both up; if
    the internal RAM left is below the firmware's floor, the kernel
    refuses the second radio's start with `BtError::Busy` rather than
    run out.
  - Light sleep with the radio on is a requirement for the default
    "while reading" policy; until it works, the policy's default is off,
    and the current is measured with a real remote before it changes.
- **The emulator** pretends, for the X4 Pro profile and on request for
  others, with made-up devices so authors and the Workshop can try
  plugins without hardware: a page-turner (an HID keyboard sending right
  and left, pressed from the emulator's window or a script step,
  `ble-key:right`) and a heart-rate sensor (service 0x180D, a
  notification each second), more as files describing a device's
  services and values. Pairing goes through the same prompt.
- **Android**, later: its BLE API (scan, `BluetoothGatt`,
  advertising), with its runtime permissions asked when a plugin with
  `Uses::BLUETOOTH` first needs them. Keyboards paired with Android
  already send key events, which the shell passes on as `hid.*`.
- **The web shell**: none (`Capabilities::bluetooth` false).
- **The plain X4 (ESP32-C3)**: not now. It has BLE, but 35 KB of
  internal RAM and 21 KB of IRAM are a large share of what reading and
  Wi-Fi leave there, and it has no PSRAM. The SDK does not depend on it:
  once the Pro's numbers are measured, a page-turner-only mode (radio
  exclusive with Wi-Fi, one link) may be tried on the X4, and adopted
  only if reading keeps its memory.

### 5. First-party plugins

- **`bluetooth`**: the policy and the settings page, modelled on
  Wi-Fi's. Its page: the radio on or off, the bonded devices (name,
  kind, connected, "Used by", forget), and "Add a device" (a scan
  showing what is found, with the strongest first, and pairing). Its
  setting "Remote" says when bonded keyboards and remotes are
  connected: while reading (a book open), while awake, or only by hand;
  reconnecting is declared work, so it happens at cheap moments and not
  in a loop. In the Saving power mode it connects only by hand.
- **Page-turners** need nothing more: the default key map (2.) and the
  keys plugin's page, where the reader rebinds `hid.*` like any button.

### 6. The interface

The typed design adds a variant to `Command` and `MenuNeed`, a field to
`Capabilities` and `Work`, and a way from `Env` to the state: changes of
types that cross, so the plugin interface is raised once (2 to 3).
Everything else (the module and its types, `Uses::BLUETOOTH`, the
`hid.*` names) is new and free. The raise lands with the first step
and goes into the next release with any other raise waiting, so authors
rebuild once.

An additive route was weighed and not taken: requests as events the
kernel listens to (`Command::Emit` with reserved ids, values as text
parameters) and the state as a service of the core plugin. It changes
no type, but it is stringly typed, invisible to the compiler and to the
API files, and turns `Emit`, a broadcast, into a request channel.

### 7. Rollout

Each step lands green on its own:

1. **SDK and kernel**: the module, `Uses::BLUETOOTH`, the commands and
   events, the kernel's arbitration, grants and pairing prompt,
   `paperpaw-hid` with the `hid.*` inputs, the interface raised. The
   emulator's made-up devices. Tests in the kernel and the SDK; the
   example plugin below built and run in the emulator.
2. **The `bluetooth` plugin**: the page, pairing, forgetting, grants,
   the "Remote" policy and the default key map, in the emulator (a
   page-turner turns pages, a heart-rate belt shows in the example).
3. **The X4 Pro**: trouble-host behind `bluetooth`, the memory budget
   and its build check, the HID host on a real remote, the current
   measured; then the capability on.
4. **Being seen**: advertising and the GATT server, in the emulator,
   then on the Pro, with a phone's BLE scanner as the other side.
5. **Android**, then the X4 decision (4.).

`docs/plugins.md` gains a section "Bluetooth" (the permission, scanning
and connecting, reading and subscribing, being seen, keys of a remote,
testing with the emulator's devices), and the skill for community
plugins learns it once step 1 is released.

### The example a plugin author writes

A heart-rate reader: it scans for the standard heart-rate service,
connects to the first found, subscribes to the measurement and shows
the beats per minute.

```rust
const FOUND: Event<BtEvent> = Event::new("pulse.found");
const LINK: Event<BtEvent> = Event::new("pulse.link");
const HEART_RATE: Uuid = Uuid::sig(0x180D);
const MEASUREMENT: Uuid = Uuid::sig(0x2A37);

fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_action(ActionSpec::new(START, "pulse.start").handler(|_, _, out| {
        out.push(bluetooth::scan(&FOUND).service(HEART_RATE).for_ms(10_000).command());
    }))?;
    r.on(FOUND, |_, event, out| {
        if let BtEvent::Found(device) = event {
            out.push(bluetooth::connect(device.id, &LINK).command());
        }
    })?;
    r.on(LINK, |env, event, out| match event {
        BtEvent::Connected { device } => out.push(bluetooth::discover(device).command()),
        BtEvent::Discovered { device, services } => {
            if let Some(c) = services.characteristic(HEART_RATE, MEASUREMENT) {
                out.push(bluetooth::subscribe(device, c.handle).command());
            }
        }
        BtEvent::Value { bytes, .. } => {
            // Flags first; the rate is a byte, or two where flag 0 says so.
            let bpm = if bytes[0] & 1 == 0 { u16::from(bytes[1]) }
                      else { u16::from_le_bytes([bytes[1], bytes[2]]) };
            out.push(Command::set(&BPM, i64::from(bpm)));
        }
        BtEvent::Failed(_) => out.push(Command::Notify(env.i18n().tr("pulse.lost").into())),
        _ => {}
    })
}
```

with `Uses::BLUETOOTH` in its manifest. In the emulator the made-up
heart-rate sensor answers it; on the X4 Pro a real belt does.

## Consequences

- Plugins get Bluetooth LE in general (scanning, GATT client and server,
  pairing through the user), testable without hardware, and the radio
  stays off unless a plugin or the reader needs it.
- Page-turners and keyboards work through the key maps with no plugin
  code, and the reader rebinds their keys like the device's buttons.
- Devices and bonds are private: a plugin sees what it found or was
  given, and pairing is always confirmed on the device.
- The plugin interface is raised once; code plugins are rebuilt with the
  release that brings it.
- The X4 Pro pays about 75 KB of flash and, while the radio is up, about
  35 KB of internal RAM plus the host's pools; the firmware's build
  checks the stack budget that the spike broke. The plain X4 waits for
  measurements.
- Battery is the open risk: the default policy stays manual until light
  sleep with the radio on works and a remote's current is measured.
