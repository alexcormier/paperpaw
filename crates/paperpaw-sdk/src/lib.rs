//! The PaperPaw plugin SDK.
//!
//! PaperPaw is a small kernel plus plugins. Every screen, action, setting,
//! key map and translation (including those PaperPaw ships with) is
//! contributed by a plugin through this SDK. Anything a built-in plugin does,
//! a community plugin can do too, and replace it.
//!
//! # Concepts
//!
//! - **Ids** ([`Id`]): every contribution has a namespaced id such as
//!   `diagnostics.screen`. The namespace is the plugin id; `core` is
//!   reserved for the kernel.
//! - **Plugins** ([`Plugin`]): a [`Manifest`] plus a `register` function that
//!   adds contributions through a [`Registrar`].
//! - **Contributions**: actions ([`ActionSpec`]), screens ([`ScreenSpec`]),
//!   slots ([`SlotSpec`]), settings ([`SettingSpec`]), fonts ([`FontSpec`]),
//!   file formats ([`FormatSpec`]), event subscriptions ([`EventHandler`]),
//!   key maps and translations (data in the PaperPaw config format).
//! - **Slots**: named UI roles such as [`builtin::slots::HOME`]. Any plugin
//!   may bind a screen to any slot; the user's configuration has the final
//!   word.
//! - **Commands** ([`Command`]): plugins never mutate kernel state directly.
//!   They read through [`Env`] and request changes with commands. This keeps
//!   the API message-based, so the same model works for sandboxed
//!   WebAssembly plugins.

#![cfg_attr(docsrs, feature(doc_cfg))]
// Without the `kernel` feature (as plugins build it) what only the
// kernel's methods use is left over; with it, as the kernel and CI build,
// unused code is found as everywhere.
#![cfg_attr(not(any(test, feature = "kernel")), allow(dead_code, unused_imports))]
#![no_std]

extern crate alloc;

mod action;
pub mod annotations;
pub mod ask;
pub mod builtin;
pub mod chrome;
pub mod collections;
mod command;
pub mod contribution;
pub mod data;
pub mod decorations;
mod env;
pub mod event;
pub mod files;
pub mod filters;
pub mod firmware;
mod fonts;
mod formats;
mod gesture;
pub mod host;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use screen::ScreenMaker;
mod hyphenation;
pub mod icons;
mod id;
pub mod install;
pub mod keys;
pub mod library;
pub mod logo;
mod memory_storage;
mod menus;
pub mod native;
pub mod numbers;
mod params;
mod plugin;
mod plugins;
pub mod power;
pub mod presets;
pub mod random;
pub mod replies;
mod screen;
pub mod secrets;
pub mod semantics;
pub mod services;
pub mod setting;
mod settings;
pub mod setup;
pub mod status;
pub mod time;
pub mod ui;
pub mod usage;
mod version;
pub mod web;
pub mod widgets;
pub mod wifi;
pub mod words;
pub mod work;

pub use action::{
    ActionHandler, ActionHint, ActionParamsHandler, ActionSpec, Actions, Observer, ObserverSpec,
};
pub use command::{
    Command, HttpOptions, HttpRequest, MAX_CLIPBOARD, ServerResponse, SocketRead, Speech,
    http_params, server_params, socket_params, speech_params,
};
pub use contribution::{Contribution, Registrar};
pub use env::{Capabilities, Env, EnvParts, Metrics};
pub use event::{Event, EventData, EventHandler};
pub use fonts::{FontFace, FontHandle, FontSet, FontSpec, Fonts};
pub use formats::{
    COVER_FIT, FormatOpener, FormatPrepared, FormatSpec, Formats, IMAGE_FIT, OpenOptions, cover_fit,
};
pub use gesture::{Direction, Gesture, GestureKind};
pub use hyphenation::{Hyphenation, HyphenationSpec};
pub use id::{Id, InvalidId};
pub use memory_storage::{MemoryStorage, read_file};
pub use menus::{MenuEntry, MenuItem, MenuNeed, MenuSection, Menus};
pub use params::Params;
pub use plugin::{Manifest, Plugin, Priority, RegisterError, SlotSpec, Uses};
pub use plugins::{PluginInfo, PluginKind, PluginProblem, PluginState, Plugins, RequiredFor};
pub use replies::{Chosen, HttpReply, ServerNotice, SocketNotice, Typed};
pub use screen::{
    CloseCause, Cx, Handled, Hint, Hints, Output, Outside, Screen, ScreenFactory, ScreenKey,
    ScreenSpec, StackEntry,
};
pub use setting::{Setting, Text};
pub use settings::{
    Choices, Level, SettingError, SettingExtra, SettingGroupSpec, SettingKind, SettingSpec,
    SettingValue, Settings,
};
pub use version::{DATA_FORMATS, SDK_VERSION, Version};

pub use paperpaw_graphics::Canvas;
/// Round shapes' geometry: angles as on a clock face, points around a
/// centre (see [`Canvas::fill_ring`], [`Canvas::draw_hand`]).
pub use paperpaw_graphics::round;
pub use paperpaw_i18n::{Arg, Translator};
pub use paperpaw_platform::{Battery, SystemStatus, WallClock};
pub use paperpaw_platform::{
    ButtonState, Color, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass,
    PixelFormat, Point, PointerEvent, PointerPhase, PointerTool, Rect, RefreshMode,
};
pub use paperpaw_platform::{DirEntry, NoStorage, Storage, StorageError};
pub use status::{
    StatusFn, StatusFrame, StatusItemSpec, StatusItems, StatusLineFn, StatusLineSpec,
    StatusSignatureFn, StatusValue,
};

/// The human-editable config format (see `paperpaw-config`), for plugins
/// that store their own data as text files.
pub mod config {
    pub use paperpaw_config::{
        Entry, ErrorKind, ParseError, Parser, Value, parse, quote, remove, set, set_all,
    };
}

/// QR codes, e.g. of an address to open on a phone (see `paperpaw-qr`;
/// [`ui::draw_qr`] draws one).
pub mod qr {
    pub use paperpaw_qr::QrCode;
}

/// The format-independent document model (see `paperpaw-document`).
pub mod document {
    pub use paperpaw_document::{
        Align, Block, BlockKind, Document, DocumentError, EmbeddedFont, ImageRef, Link, Locator,
        MAX_FAMILIES, Metadata, OffsetMap, Run, TextStyle, TocEntry, is_external,
    };
}

/// Page layout engine (see `paperpaw-layout`), usable by any reader
/// plugin. Unstable until 1.0: it still changes as readers need it, so it
/// comes only with the `unstable` feature.
#[cfg(feature = "unstable")]
pub mod layout {
    pub use paperpaw_layout::{
        Face, Hyphenate, LayoutContext, LineRun, Page, PageImage, PageLine, Style, Typesetter,
        VERSION, layout_page, line_start,
    };
}

/// Streaming XML/XHTML tokenizer (see `paperpaw-xml`) for format plugins.
pub mod xml {
    pub use paperpaw_xml::{Attrs, Token, Tokenizer, decode, escape, local_name};
}

/// Streaming JPEG, PNG and GIF decoding to gray or colour (see
/// `paperpaw-image`).
pub mod image {
    pub use paperpaw_image::{
        ColorFile, ColorWriter, Downscaler, Fit, Format, GrayFile, GrayWriter, ImageError,
        ImageFile, ImageSink, PngWriter, Source, decode, dimensions, luma,
    };

    use crate::document::ImageRef;
    use paperpaw_platform::Storage;

    /// Reads a file in storage as an image source.
    struct FileSource<'a> {
        storage: &'a dyn Storage,
        path: &'a str,
        offset: u64,
    }

    impl Source for FileSource<'_> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, ImageError> {
            let n = self
                .storage
                .read_at(self.path, self.offset, buf)
                .map_err(|_| ImageError::Io)?;
            self.offset += n as u64;
            Ok(n)
        }
    }

    /// Decodes the JPEG or PNG file `from` into the gray file `to`, no
    /// larger than `fit` allows, for
    /// [`ui::draw_image`](crate::ui::draw_image); `None` if it is no image
    /// PaperPaw reads.
    pub fn gray_from_file(
        storage: &dyn Storage,
        from: &str,
        to: &str,
        fit: Fit,
    ) -> Option<ImageRef> {
        picture_from_file(storage, from, to, fit, false)
    }

    /// [`gray_from_file`], or in colour (for colour screens) with `color`.
    pub fn picture_from_file(
        storage: &dyn Storage,
        from: &str,
        to: &str,
        fit: Fit,
        color: bool,
    ) -> Option<ImageRef> {
        let mut head = [0u8; 8];
        let n = storage.read_at(from, 0, &mut head).ok()?;
        let format = Format::detect(&head[..n])?;
        let mut source = FileSource {
            storage,
            path: from,
            offset: 0,
        };
        if color {
            let mut writer = ColorWriter::new(storage, to);
            decode(format, &mut source, fit, &mut writer)
                .and_then(|()| writer.finish())
                .ok()?;
        } else {
            let mut writer = GrayWriter::new(storage, to);
            decode(format, &mut source, fit, &mut writer)
                .and_then(|()| writer.finish())
                .ok()?;
        }
        ImageFile::open(storage, to).map(|file| ImageRef {
            path: file.path,
            width: file.width,
            height: file.height,
        })
    }
}

/// Text and font helpers (see `paperpaw-text`), e.g. reading a font
/// file's family.
pub mod text {
    pub use paperpaw_text::{OUTLINES, sfnt};
}

/// Hyphenation with compiled patterns (see `paperpaw-hyphen`).
pub mod hyphen {
    pub use paperpaw_hyphen::{BLOCK, InvalidPatterns, Patterns, compile};
}

/// Streaming ZIP reader (see `paperpaw-zip`) for ZIP-based formats.
/// Unstable until 1.0, so it comes only with the `unstable` feature.
#[cfg(feature = "unstable")]
pub mod zip {
    pub use paperpaw_zip::{Entry, EntryReader, Method, ZipArchive, ZipError};
}

/// JSON reading and writing without building a tree (see
/// `paperpaw-json`), e.g. for web services.
pub mod json {
    pub use paperpaw_json::{
        Value, number, object, raw_member, stream, string, string_array, text, unquote,
    };
}

/// Hashes and hex (see `paperpaw-digest`): FNV-1a for names of files kept
/// per book, MD5, SHA-1 and SHA-256 for what services and files ask.
pub mod digest {
    pub use paperpaw_digest::{fnv1a64, hex, md5, sha1, sha256};
}
pub use paperpaw_text::{Font, FontError, LineMetrics};
