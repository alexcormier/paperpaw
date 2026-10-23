//! Answers recorded for plugins' requests (`network`, `fixture`), and
//! the files they are read from.

use std::path::Path;

use super::MAX_FIXTURE;

/// Where plugins' requests go (`network`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    /// To the network, as without the protocol.
    Real,
    /// To the answers recorded (`fixture`); one with none, failed.
    Recorded,
    /// Nowhere: every one fails, as without a network.
    Offline,
}

impl Network {
    pub(super) fn name(self) -> &'static str {
        match self {
            Network::Real => "real",
            Network::Recorded => "recorded",
            Network::Offline => "offline",
        }
    }
}

/// An answer recorded for plugins' requests (`fixture`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    /// The address it answers; ending in `*`, every one that starts so.
    pub url: String,
    /// The method it answers (any where `None`).
    pub method: Option<String>,
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
}

impl Fixture {
    pub(super) fn answers(&self, method: &str, url: &str) -> bool {
        let address = match self.url.strip_suffix('*') {
            Some(start) => url.starts_with(start),
            None => url == self.url,
        };
        address
            && self
                .method
                .as_deref()
                .is_none_or(|m| m.eq_ignore_ascii_case(method))
    }
}

/// The bytes of `file` for a `fixture`: a file (no pipe or device) of at
/// most [`MAX_FIXTURE`] bytes in `folder` (relative to it), where it is
/// once links are followed.
pub(super) fn fixture_file(folder: &Path, file: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let shown = file.display();
    let folder = folder
        .canonicalize()
        .map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = folder
        .join(file)
        .canonicalize()
        .map_err(|e| format!("{shown}: {e}"))?;
    if !path.starts_with(&folder) {
        return Err(format!(
            "{shown}: not in the folder the device was started on"
        ));
    }
    let large = || format!("{shown}: larger than {} MB", MAX_FIXTURE >> 20);
    // Looked at before it is opened: opening a pipe waits for a writer.
    let meta = std::fs::metadata(&path).map_err(|e| format!("{shown}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{shown}: not a file"));
    }
    if meta.len() > MAX_FIXTURE {
        return Err(large());
    }
    let opened = std::fs::File::open(&path).map_err(|e| format!("{shown}: {e}"))?;
    if !opened.metadata().is_ok_and(|m| m.is_file()) {
        return Err(format!("{shown}: not a file"));
    }
    let mut body = Vec::new();
    opened
        .take(MAX_FIXTURE + 1)
        .read_to_end(&mut body)
        .map_err(|e| format!("{shown}: {e}"))?;
    if body.len() as u64 > MAX_FIXTURE {
        return Err(large());
    }
    Ok(body)
}

/// Answers plugins' requests as the protocol's `network` says: from the
/// answers recorded, or not at all.
pub(super) struct Answers(
    pub(super) std::rc::Rc<std::cell::RefCell<(Option<Network>, Vec<Fixture>)>>,
);

impl crate::http::Route for Answers {
    fn start(&mut self, call: paperpaw_core::HttpCall) -> Option<paperpaw_core::HttpResponse> {
        use paperpaw_core::sdk::HttpError::{NoNetwork, Refused};
        let (mode, fixtures) = &*self.0.borrow();
        let failed = |error, why: &str| paperpaw_core::HttpResponse::failed(call.id, error, why);
        Some(match mode {
            Some(Network::Recorded) => match fixtures
                .iter()
                .rev()
                .find(|f| f.answers(&call.method, &call.url))
            {
                Some(fixture) => paperpaw_core::HttpResponse {
                    id: call.id,
                    status: fixture.status,
                    body: fixture.body.clone(),
                    content_type: fixture.content_type.clone(),
                    ..paperpaw_core::HttpResponse::default()
                },
                None => failed(Refused, "no answer recorded for this request"),
            },
            _ => failed(NoNetwork, "offline"),
        })
    }

    fn later(&mut self) -> Option<paperpaw_core::HttpResponse> {
        None
    }
}
