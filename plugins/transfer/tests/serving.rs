//! Transfer by browser with the real kernel and its server: the screen
//! starts a server and shows the address with its key; requests come in
//! as a shell sends them (an upload's body in parts), and what the
//! plugin answers and writes is checked: an upload in many parts lands
//! whole, uploads out of reach are refused and leave nothing behind,
//! names taken get a number, a wrong key gets nothing, a lost
//! connection leaves no half file, and closing the screen stops the
//! server.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::rc::Rc;

use paperpaw_core::{Effects, Io, Kernel, ServerCall, ServerEvent};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_transfer::Transfer;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{DirEntry, Id, MemoryStorage, Storage, StorageError};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test-touch",
    name: "Test touch",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: false,
        touch: true,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Small,
};

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>);

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

/// An answer the plugin gave: its status and body.
#[derive(Debug)]
struct Answer {
    status: u16,
    body: Vec<u8>,
    file: Option<String>,
}

/// A random source for the tests: bytes that differ, the same each run.
fn fill(buf: &mut [u8]) -> bool {
    for (i, byte) in buf.iter_mut().enumerate() {
        *byte = (i as u8).wrapping_mul(97).wrapping_add(13);
    }
    true
}

struct Device {
    kernel: Kernel,
    storage: Shared,
    now: u64,
    server: u32,
    key: String,
    next_request: u32,
}

impl Device {
    /// The transfer screen open and its server started.
    fn serving() -> Self {
        let storage = Shared::default();
        storage.create_dir("Books").unwrap();
        storage.write("Books/a.epub", b"old").unwrap();
        let kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .user_config("[core]\nlanguage = en\n")
            .network(true)
            // The key is drawn from the platform's random source.
            .random(fill)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Transfer)
            .build()
            .unwrap();
        let mut device = Self {
            kernel,
            storage,
            now: 0,
            server: 0,
            key: String::new(),
            next_request: 1,
        };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        device.run(Id::new_static("transfer.open"));
        assert_eq!(
            device.kernel.active_screen().map(Id::as_str),
            Some("transfer.screen")
        );
        let calls = device.calls();
        let Some(ServerCall::Listen { server, port }) = calls.first() else {
            panic!("no server: {calls:?}");
        };
        assert_eq!(*port, 8080);
        device.server = *server;
        device.event(ServerEvent::Started {
            server: device.server,
            address: "192.168.1.20:8080".into(),
        });
        // The key, from the address on the screen.
        device.kernel.render(&mut |_, _| {});
        let shown: Vec<String> = device
            .kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect();
        let address = shown
            .iter()
            .find(|l| l.contains("k="))
            .unwrap_or_else(|| panic!("no address: {shown:?}"));
        device.key = address.rsplit("k=").next().unwrap().to_string();
        // 13 letters of 5 bits: 65 bits.
        assert_eq!(device.key.len(), 13, "{address}");
        device
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn calls(&mut self) -> Vec<ServerCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Server(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    fn event(&mut self, event: ServerEvent) {
        self.now += 10;
        let effects = self.kernel.server_event(Instant(self.now), event);
        self.settle(effects);
    }

    /// Sends a request, with a body in `parts` if any, and returns the
    /// answer.
    fn request(&mut self, method: &str, target: &str, parts: Option<&[&[u8]]>) -> Answer {
        let request = self.next_request;
        self.next_request += 1;
        self.event(ServerEvent::Request {
            server: self.server,
            request,
            method: method.into(),
            target: target.into(),
            body: parts.is_some(),
        });
        if let Some(parts) = parts {
            for (i, part) in parts.iter().enumerate() {
                self.event(ServerEvent::Body {
                    request,
                    data: part.to_vec(),
                    last: i + 1 == parts.len(),
                });
            }
        }
        self.answer(request)
    }

    fn answer(&mut self, request: u32) -> Answer {
        let calls = self.calls();
        calls
            .into_iter()
            .find_map(|call| match call {
                ServerCall::Respond {
                    request: r,
                    status,
                    body,
                    file,
                    ..
                } if r == request => Some(Answer {
                    status,
                    body,
                    file: file.map(|f| f.path),
                }),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no answer to request {request}"))
    }

    fn upload(&mut self, path: &str, parts: &[&[u8]]) -> Answer {
        let target = format!("/api/file?k={}&path={path}", self.key);
        self.request("PUT", &target, Some(parts))
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.storage.0.get(path)
    }

    /// The files waiting in the kernel's folder for bodies.
    fn waiting_bodies(&self) -> usize {
        let mut count = 0;
        let _ = self.storage.list(".paperpaw/tmp", &mut |_| count += 1);
        count
    }

    /// What the screen says happened last.
    fn says(&mut self, text: &str) -> bool {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .iter()
            .any(|p| p.label.contains(text))
    }
}

#[test]
fn the_page_needs_the_key_shown() {
    let mut device = Device::serving();
    assert_eq!(device.request("GET", "/", None).status, 403);
    assert_eq!(device.request("GET", "/?k=00000000", None).status, 403);
    let target = format!("/?k={}", device.key);
    let page = device.request("GET", &target, None);
    assert_eq!(page.status, 200);
    let html = String::from_utf8(page.body).unwrap();
    // With its texts in the device's language.
    assert!(!html.contains("/*STRINGS*/{}"));
    assert!(
        html.contains("<html") || html.contains("<!doctype"),
        "{html}"
    );
}

#[test]
fn an_upload_in_many_parts_lands_whole() {
    let mut device = Device::serving();
    let data: Vec<u8> = (0..3 * 1024 * 1024u32).map(|i| (i % 253) as u8).collect();
    let parts: Vec<&[u8]> = data.chunks(64 * 1024).collect();
    let answer = device.upload("Books/big%20book.pdf", &parts);
    assert_eq!(
        answer.status,
        200,
        "{:?}",
        String::from_utf8_lossy(&answer.body)
    );
    assert_eq!(device.read("Books/big book.pdf").unwrap(), data);
    assert_eq!(device.waiting_bodies(), 0);
    assert!(device.says("big book.pdf"));
}

#[test]
fn uploads_out_of_reach_are_refused_and_leave_nothing() {
    let mut device = Device::serving();
    for path in [
        "../x.epub",
        "%2E%2E%2Fx.epub",
        "Books\\..\\..\\x.epub",
        "C:\\x.epub",
        ".paperpaw/secrets.txt",
        "Books/.x.epub",
        "Books/%00.epub",
    ] {
        let answer = device.upload(path, &[b"evil"]);
        assert_eq!(answer.status, 400, "{path}");
        // The body saved for it is gone again.
        assert_eq!(device.waiting_bodies(), 0, "{path}");
    }
    assert!(!device.storage.exists("x.epub"));
    assert!(!device.storage.exists(".paperpaw/secrets.txt"));
    assert_eq!(device.read("Books/a.epub").unwrap(), b"old");
}

#[test]
fn a_name_taken_gets_a_number() {
    let mut device = Device::serving();
    assert_eq!(device.upload("Books/a.epub", &[b"new"]).status, 200);
    assert_eq!(device.upload("Books/a.epub", &[b"newer"]).status, 200);
    assert_eq!(device.read("Books/a.epub").unwrap(), b"old");
    assert_eq!(device.read("Books/a (2).epub").unwrap(), b"new");
    assert_eq!(device.read("Books/a (3).epub").unwrap(), b"newer");
    assert!(device.says("a (3).epub"));
}

#[test]
fn a_wrong_key_writes_nothing() {
    let mut device = Device::serving();
    let answer = device.request(
        "PUT",
        "/api/file?k=wrongkey&path=Books/x.epub",
        Some(&[b"x"]),
    );
    assert_eq!(answer.status, 403);
    assert!(!device.storage.exists("Books/x.epub"));
    assert_eq!(device.waiting_bodies(), 0);
}

#[test]
fn a_lost_connection_leaves_no_half_file() {
    let mut device = Device::serving();
    let request = device.next_request;
    device.next_request += 1;
    let target = format!("/api/file?k={}&path=Books/half.epub", device.key);
    device.event(ServerEvent::Request {
        server: device.server,
        request,
        method: "PUT".into(),
        target,
        body: true,
    });
    device.event(ServerEvent::Body {
        request,
        data: vec![1; 1000],
        last: false,
    });
    device.event(ServerEvent::Closed { request });
    assert!(!device.storage.exists("Books/half.epub"));
    assert_eq!(device.waiting_bodies(), 0);
    // Parts after the end are ignored.
    device.event(ServerEvent::Body {
        request,
        data: vec![2; 10],
        last: true,
    });
    assert!(device.calls().is_empty());
}

#[test]
fn files_are_sent_listed_and_deleted() {
    let mut device = Device::serving();
    let key = device.key.clone();
    let sent = device.request("GET", &format!("/api/file?k={key}&path=Books/a.epub"), None);
    assert_eq!(
        (sent.status, sent.file.as_deref()),
        (200, Some("Books/a.epub"))
    );
    let list = device.request("GET", &format!("/api/list?k={key}&path=Books"), None);
    assert_eq!(
        String::from_utf8(list.body).unwrap(),
        r#"{"entries":[{"name":"a.epub","dir":false,"size":3}]}"#
    );
    // The top lists no hidden folders.
    let top = device.request("GET", &format!("/api/list?k={key}"), None);
    assert!(!String::from_utf8(top.body).unwrap().contains(".paperpaw"));
    let made = device.request("POST", &format!("/api/mkdir?k={key}&path=Books/New"), None);
    assert_eq!(made.status, 200);
    let deleted = device.request(
        "POST",
        &format!("/api/delete?k={key}&path=Books/a.epub"),
        None,
    );
    assert_eq!(deleted.status, 200);
    assert!(!device.storage.exists("Books/a.epub"));
    assert!(device.says("a.epub"));
}

#[test]
fn without_a_network_the_screen_says_so() {
    let mut device = Device::serving();
    device.event(ServerEvent::Failed {
        server: device.server,
        reason: "no network".into(),
    });
    device.kernel.render(&mut |_, _| {});
    let shown: Vec<String> = device
        .kernel
        .describe()
        .into_iter()
        .map(|p| p.label)
        .collect();
    assert!(!shown.iter().any(|l| l.contains("k=")), "{shown:?}");
}

#[test]
fn closing_the_screen_stops_the_server() {
    let mut device = Device::serving();
    device.run(actions::BACK);
    let calls = device.calls();
    assert!(
        calls
            .iter()
            .any(|c| matches!(c, ServerCall::Stop { server } if *server == device.server)),
        "{calls:?}"
    );
    // Requests after it are not answered by the plugin.
    let request = device.next_request;
    device.event(ServerEvent::Request {
        server: device.server,
        request,
        method: "GET".into(),
        target: format!("/api/list?k={}", device.key),
        body: false,
    });
    assert!(device.calls().is_empty());
}
