//! Setting up Wi-Fi with a phone, with the real kernel: the device looks
//! for networks, opens its own, serves the page there, and joins the
//! network chosen on the phone, keeping its password; a network that
//! does not take the password opens the own network again. And networks
//! put on the card in `wifi.ini` are kept, the file removed.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_core::{Io, Kernel, ServerCall, ServerEvent, WifiEvent};
use paperpaw_platform::Rotation;
use paperpaw_plugin_wifi::{PHONE, Wifi};
use paperpaw_sdk::secrets::{FILE, Secrets};
use paperpaw_sdk::wifi::{self, JoinFailure, WifiCommand, WifiNetwork, WifiState};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, Params, PixelFormat,
    Storage,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Gray4,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Small,
};

fn random(buf: &mut [u8]) -> bool {
    buf.iter_mut()
        .enumerate()
        .for_each(|(i, b)| *b = i as u8 * 37);
    true
}

struct Test {
    kernel: Kernel,
    at: u64,
}

impl Test {
    fn new(storage: MemoryStorage) -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .network(true)
            .wifi(true)
            .random(random)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Wifi)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let _ = kernel.take_io();
        Test { kernel, at: 0 }
    }

    fn now(&mut self, ms: u64) -> Instant {
        self.at += ms;
        Instant(self.at)
    }

    fn io(&mut self) -> Vec<Io> {
        self.kernel.take_io()
    }

    fn radio(io: &[Io]) -> Vec<WifiCommand> {
        io.iter()
            .filter_map(|io| match io {
                Io::Wifi(command) => Some(command.clone()),
                _ => None,
            })
            .collect()
    }

    fn wifi(&mut self, event: WifiEvent) -> Vec<Io> {
        let now = self.now(100);
        self.kernel.wifi_event(now, event);
        self.io()
    }

    fn server(&mut self, event: ServerEvent) -> Vec<Io> {
        let now = self.now(100);
        self.kernel.server_event(now, event);
        self.io()
    }

    fn tick(&mut self, ms: u64) -> Vec<Io> {
        let now = self.now(ms);
        self.kernel.tick(now);
        self.io()
    }

    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

fn network(ssid: &str) -> WifiNetwork {
    WifiNetwork {
        ssid: String::from(ssid),
        signal: -50,
        secured: true,
    }
}

/// The page's answer to a request: its status and body.
fn answered(io: &[Io]) -> (u16, String) {
    io.iter()
        .find_map(|io| match io {
            Io::Server(ServerCall::Respond { status, body, .. }) => {
                Some((*status, String::from_utf8_lossy(body).into_owned()))
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_network_chosen_on_the_phone_is_joined_and_kept() {
    let mut t = Test::new(MemoryStorage::new());
    let now = t.now(100);
    t.kernel.open_screen(now, PHONE, Params::new());
    let io = t.io();
    assert_eq!(Test::radio(&io), [WifiCommand::On, WifiCommand::Scan]);
    t.wifi(WifiEvent::State(WifiState::On));
    // Found: the own network opens.
    let io = t.wifi(WifiEvent::Scanned(vec![network("Home"), network("Cafe")]));
    let Some(WifiCommand::Host { ssid, password }) = Test::radio(&io).pop() else {
        panic!("{io:?}");
    };
    assert!(ssid.starts_with("PaperPaw-"), "{ssid}");
    assert!(password.len() >= 8);
    // Open: the page is served there (the kernel lets the server go to
    // the shell without a way out).
    let io = t.wifi(WifiEvent::State(WifiState::Hosting {
        ssid: ssid.clone(),
        address: String::from("192.168.4.1"),
    }));
    let server = io
        .iter()
        .find_map(|io| match io {
            Io::Server(ServerCall::Listen { server, port: 80 }) => Some(*server),
            _ => None,
        })
        .unwrap();
    t.server(ServerEvent::Started {
        server,
        address: String::from("192.168.4.1:80"),
    });
    let shown = t.shown();
    assert!(shown.contains(&password), "{shown}");
    assert!(shown.contains("http://192.168.4.1/"), "{shown}");
    // The page lists what was found.
    let io = t.server(ServerEvent::Request {
        server,
        request: 1,
        method: String::from("GET"),
        target: String::from("/"),
        body: false,
    });
    let (status, page) = answered(&io);
    assert_eq!(status, 200);
    assert!(
        page.contains("value=\"Home\"") && page.contains("value=\"Cafe\""),
        "{page}"
    );
    // Chosen: answered first, then the own network closes for that one.
    t.server(ServerEvent::Request {
        server,
        request: 2,
        method: String::from("POST"),
        target: String::from("/join"),
        body: true,
    });
    let io = t.server(ServerEvent::Body {
        request: 2,
        data: b"ssid=Home&other=&password=s3cret".to_vec(),
        last: true,
    });
    let (status, page) = answered(&io);
    assert_eq!(status, 200);
    assert!(page.contains("Home"), "{page}");
    assert!(Test::radio(&io).is_empty());
    let io = t.tick(2000);
    assert_eq!(
        Test::radio(&io),
        [WifiCommand::Join {
            ssid: String::from("Home"),
            password: String::from("s3cret")
        }]
    );
    t.wifi(WifiEvent::State(WifiState::Joining {
        ssid: String::from("Home"),
    }));
    t.wifi(WifiEvent::State(WifiState::Online {
        ssid: String::from("Home"),
        address: String::from("10.0.0.5"),
    }));
    let secrets =
        Secrets::decode(&String::from_utf8(t.kernel.storage().read_to_vec(FILE)).unwrap());
    assert!(wifi::known(secrets.iter()).contains(&(String::from("Home"), String::from("s3cret"))));
    assert!(t.shown().contains("Connected to Home"));
}

#[test]
fn a_password_not_taken_opens_the_own_network_again() {
    let mut t = Test::new(MemoryStorage::new());
    let now = t.now(100);
    t.kernel.open_screen(now, PHONE, Params::new());
    t.io();
    t.wifi(WifiEvent::State(WifiState::On));
    let io = t.wifi(WifiEvent::Scanned(vec![network("Home")]));
    let host = Test::radio(&io);
    let Some(WifiCommand::Host { ssid, .. }) = host.first() else {
        panic!("{io:?}");
    };
    let io = t.wifi(WifiEvent::State(WifiState::Hosting {
        ssid: ssid.clone(),
        address: String::from("192.168.4.1"),
    }));
    let Some(Io::Server(ServerCall::Listen { server, .. })) = io.first().cloned() else {
        panic!("{io:?}");
    };
    t.server(ServerEvent::Request {
        server,
        request: 1,
        method: String::from("POST"),
        target: String::from("/join"),
        body: true,
    });
    t.server(ServerEvent::Body {
        request: 1,
        data: b"ssid=Home&password=wrong".to_vec(),
        last: true,
    });
    t.tick(2000);
    let io = t.wifi(WifiEvent::Failed {
        ssid: String::from("Home"),
        reason: JoinFailure::Password,
    });
    assert_eq!(Test::radio(&io), host);
    assert!(t.shown().contains("Wrong password for Home"));
}

#[test]
fn networks_on_the_card_are_kept_and_the_file_goes() {
    let storage = MemoryStorage::new().with(
        "wifi.ini",
        "# from the flasher\nssid = Home Net\npassword = pw 1\n\nssid = Cafe\n",
    );
    let t = Test::new(storage);
    let storage = t.kernel.storage();
    assert!(!storage.exists("wifi.ini"));
    let secrets = Secrets::decode(&String::from_utf8(storage.read_to_vec(FILE)).unwrap());
    let known = wifi::known(secrets.iter());
    assert!(known.contains(&(String::from("Home Net"), String::from("pw 1"))));
    assert!(known.contains(&(String::from("Cafe"), String::new())));
}

trait ReadAll {
    fn read_to_vec(&self, path: &str) -> Vec<u8>;
}

impl ReadAll for dyn Storage + '_ {
    fn read_to_vec(&self, path: &str) -> Vec<u8> {
        paperpaw_sdk::read_file(self, path, 1 << 20).unwrap()
    }
}
