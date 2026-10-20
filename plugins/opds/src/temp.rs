//! The files answers are written to: each request gets one of its own,
//! so two answers that arrive together (two books tapped one after the
//! other, a feed and a search description) never write into one file.
//! A file goes once its answer is handled; what is left (the device was
//! switched off during a download, a screen was closed before its answer
//! came, a sync was stopped) goes at the next start.

use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use core::cell::Cell;

use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{Env, Storage, files};

use crate::PLUGIN;

/// Parameter of a request: the file its answer is written to.
pub(crate) const FILE: &str = "file";

/// The files used by versions before this one, one for each kind of
/// request.
const FIXED: [&str; 5] = [
    "browse.xml",
    "download.tmp",
    "cover.tmp",
    "sync.xml",
    "sync.tmp",
];

fn dir() -> String {
    format!("{}/tmp", Env::plugin_dir(PLUGIN.as_str()))
}

/// Names the files: one counter for the plugin's screens and its sync.
#[derive(Debug, Clone, Default)]
pub(crate) struct TempFiles(Rc<Cell<u32>>);

impl TempFiles {
    /// A file for the answer to a new request, ending in `ext`.
    pub(crate) fn next(&self, ext: &str) -> String {
        let n = self.0.get().wrapping_add(1);
        self.0.set(n);
        format!("{}/{n}.{ext}", dir())
    }
}

/// The file `reply` was written to, if it is one of these.
pub(crate) fn file(reply: &HttpReply) -> Option<&str> {
    let path = reply.context.get(FILE)?;
    let name = path.strip_prefix(&dir())?.strip_prefix('/')?;
    files::is_plain_name(name).then_some(path)
}

/// Removes the file `reply` was written to, if it is still there (one
/// moved into the library is not).
pub(crate) fn discard(storage: &dyn Storage, reply: &HttpReply) {
    if let Some(path) = file(reply) {
        let _ = storage.remove(path);
    }
}

/// Removes what was left before this start.
pub(crate) fn clean(storage: &dyn Storage) {
    let _ = files::delete(storage, &dir());
    let plugin = Env::plugin_dir(PLUGIN.as_str());
    for name in FIXED {
        let _ = storage.remove(&format!("{plugin}/{name}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::Params;

    fn reply(file: &str) -> HttpReply {
        HttpReply {
            status: 200,
            body: String::new(),
            etag: None,
            content_type: None,
            final_url: None,
            context: Params::new().with(FILE, file),
            refused: None,
        }
    }

    #[test]
    fn every_request_has_a_file_of_its_own() {
        let temp = TempFiles::default();
        let shared = temp.clone();
        let (a, b) = (temp.next("tmp"), shared.next("tmp"));
        assert_ne!(a, b);
        assert_eq!(file(&reply(&a)), Some(a.as_str()));
        // Only the plugin's own files are ever removed.
        assert_eq!(file(&reply("books/a.epub")), None);
        assert_eq!(file(&reply(&format!("{}/../a.epub", dir()))), None);
    }
}
