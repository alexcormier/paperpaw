//! The covers of a feed's entries, where the device has the memory for
//! them: loaded one after the other for the page shown, decoded and
//! kept, so they show at once the next time.

use super::*;

/// The answer with a cover.
pub(super) const COVER: Event<HttpReply> = Event::new("opds.cover");

/// Parameter: the entry a cover belongs to.
pub(super) const ENTRY: &str = "entry";

pub(super) fn cover_download_path() -> String {
    format!(
        "{}/cover.tmp",
        quireink_sdk::Env::plugin_dir(PLUGIN.as_str())
    )
}

/// Where the cover from `url` is kept, decoded.
pub(super) fn cover_path(url: &str) -> String {
    format!(
        "{}/covers/{}.gray",
        quireink_sdk::Env::plugin_dir(PLUGIN.as_str()),
        quireink_digest::md5::of(url)
    )
}

impl BrowseScreen {
    /// The row of entry `i`, if it is on the page shown.
    pub(super) fn row_of(&self, i: usize) -> Option<usize> {
        let row = self.rows().iter().position(|r| *r == Row::Entry(i))?;
        let rows = self.list.rows();
        (row / rows == self.list.page()).then_some(row % rows)
    }

    /// Loads the cover of the next entry on the page shown that has one
    /// and lacks it; covers kept from before need no download.
    pub(super) fn next_cover(&mut self, cx: &mut Cx<'_, '_>) {
        if !self.with_covers || self.fetching.is_some() {
            return;
        }
        let env = *cx.env();
        for i in 0..self.feed.entries.len() {
            if self.covers[i].is_some() || self.failed.contains(&i) || self.row_of(i).is_none() {
                continue;
            }
            let Some(url) = self.feed.entries[i].thumbnail.clone() else {
                continue;
            };
            let kept = cover_path(&url);
            if let Some(file) = quireink_sdk::image::ImageFile::open(env.storage, &kept) {
                self.covers[i] = Some(ImageRef {
                    path: file.path,
                    width: file.width,
                    height: file.height,
                });
                continue;
            }
            if !env.caps.network {
                return;
            }
            self.fetching = Some(i);
            let params = Params::new()
                .with(URL, url.clone())
                .with(ENTRY, i.to_string());
            cx.command(Command::Http(request(
                &self.catalog,
                &url,
                cover_download_path(),
                &COVER,
                params,
            )));
            return;
        }
        // Covers found in the cache show at once.
        self.redraw(cx);
    }

    /// A cover arrived: decoded and kept, then the next one.
    pub(super) fn cover_loaded(&mut self, reply: &HttpReply, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let i = reply
            .context
            .get(ENTRY)
            .and_then(|i| i.parse::<usize>().ok());
        let url = reply.context.get(URL).unwrap_or_default().to_string();
        let Some(i) = i.filter(|&i| self.fetching == Some(i) && i < self.covers.len()) else {
            return;
        };
        self.fetching = None;
        let tmp = cover_download_path();
        let side = self.style.noted_row();
        let image = (200..300)
            .contains(&reply.status)
            .then(|| {
                let color = env.device.display.format.is_color();
                quireink_sdk::image::picture_from_file(
                    env.storage,
                    &tmp,
                    &cover_path(&url),
                    quireink_sdk::image::Fit::side(side),
                    color,
                )
            })
            .flatten();
        let _ = env.storage.remove(&tmp);
        match image {
            Some(image) => {
                self.covers[i] = Some(image);
                if let Some(row) = self.row_of(i) {
                    cx.request_redraw_area(self.list.row_rect(row));
                    cx.request_refresh(RefreshMode::Fast);
                }
            }
            None => self.failed.push(i),
        }
        self.next_cover(cx);
    }
}
