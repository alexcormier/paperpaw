//! The pictures of the moods: one's own from the folder
//! [`PICTURES`](crate::PICTURES) names, else PaperPaw's cats, carried in
//! the plugin. Each is decoded once into the plugin's folder (a few rows
//! at a time, never whole in memory) and drawn or made small from there.

use alloc::format;
use alloc::string::String;

use paperpaw_sdk::Env;
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::image::{Fit, ImageFile};

use crate::{Mood, PICTURES, data};

/// The cats: 16 grays, at most 400 pixels a side, white around them.
const CATS: [&[u8]; 4] = [
    include_bytes!("../pictures/sad.png"),
    include_bytes!("../pictures/meh.png"),
    include_bytes!("../pictures/content.png"),
    include_bytes!("../pictures/happy.png"),
];

/// The largest side a picture is decoded to: as large as it is drawn on
/// a screen.
const MOST: u32 = 800;

/// The picture of `mood`.
pub(crate) fn of(env: &Env<'_>, mood: Mood) -> Option<ImageRef> {
    own(env, mood).or_else(|| cat(env, mood))
}

fn opened(env: &Env<'_>, file: &str) -> Option<ImageRef> {
    ImageFile::open(env.storage(), file).map(|f| ImageRef {
        path: f.path,
        width: f.width,
        height: f.height,
    })
}

/// One's own picture of `mood`, if the folder has one PaperPaw reads.
fn own(env: &Env<'_>, mood: Mood) -> Option<ImageRef> {
    let folder: String = env
        .settings
        .text(PICTURES.as_str())
        .trim_matches('/')
        .into();
    if folder.is_empty() {
        return None;
    }
    for extension in ["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"] {
        let path = format!("{folder}/{}.{extension}", mood.name());
        let Ok(size) = env.storage().size(&path) else {
            continue;
        };
        // Made again where the picture changed (another size, mostly).
        let file = data(&format!("own-{}-{size}.gray", mood.name()));
        if let Some(image) = opened(env, &file) {
            return Some(image);
        }
        let fit = Fit::side(MOST);
        if let Some(image) =
            paperpaw_sdk::image::picture_from_file(env.storage(), &path, &file, fit, false)
        {
            return Some(image);
        }
    }
    None
}

/// PaperPaw's cat of `mood`.
fn cat(env: &Env<'_>, mood: Mood) -> Option<ImageRef> {
    let bytes = CATS[mood as usize];
    // Named by its size: a cat drawn anew in an update is decoded anew.
    let file = data(&format!("cat-{}-{}.gray", mood.name(), bytes.len()));
    opened(env, &file).or_else(|| {
        paperpaw_sdk::image::picture_from_bytes(env.storage(), bytes, &file, Fit::side(MOST), false)
    })
}
