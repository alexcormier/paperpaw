# Publishing a plugin in the PaperPaw store

How to guide an author, step by step, from a finished plugin to its
listing in the plugin library on paperpaw.org, which the website and
every device's store (*More plugins*) read. It is written for an AI
assistant that helps the author. `docs/plugins.md` ("Publishing")
explains the same for people; ADR 0023 (the store) and ADR 0024 (code
plugins as files) say why it works so.

Names, commands, fields and messages below are taken from the
repositories. Check them before relying on them (`paperpaw-sign
--help`, `samples/hello-native`, the catalogue's rules on paperpaw.org).
Where this guide says **to be confirmed**, the website has not settled
it yet: look at the live form or ask the user, and never fill the gap
with a guess.

## 0. Rules for the assistant

You MUST NOT:

- **Show, copy or move the secret key.** Never print the key file
  (`cat`, `type`, `Get-Content`, an editor), never paste it into the
  chat, a file in the repository, an issue, a pull request, a log, a
  form field or a command line. Refer to it only by its path, as
  `--key <path>`. If its contents appear anywhere they should not, stop
  and tell the user that the key is compromised (see section 10).
- **Commit the key.** Before every commit, check that no `*.key` file
  and no 64-digit secret is staged (`git status`, `git diff --cached`).
- **Send anything without the user's explicit yes for that step**:
  uploading in the form, submitting for moderation, opening a pull
  request, pushing a branch or tag, creating a release, setting a
  repository secret. Show what will be sent first (the files, the
  catalogue entry) and wait.
- **Invent fields, URLs or messages.** The catalogue refuses any field it
  does not know. Use only the fields in section 6; where a field or
  address is to be confirmed, ask.
- **Get around a check.** Never change an `abi` or `sha256` to match a
  file, strip or forge a signature, edit a `.ppn` by hand, or rename a
  file built for one device to another. Fix the cause (section 9).
- **Sign with a new key for an update** unless the user decided to change
  keys knowing that every reader who has the plugin will be warned.
- **Publish what the user may not publish**: code or files under a
  licence that forbids it, someone else's plugin as theirs, secrets
  (passwords, API tokens) inside the plugin's files or code.
- **Claim a result you have not seen**: that a check passed, that the
  plugin is listed, that it is "Verified". Report what the site or the
  tool said, word for word.

You SHOULD: work through sections 1 to 8 in order, run every check
yourself where you can, and end with the checklist in section 11.

## 1. What gets published

| | Data plugin | Code plugin |
|---|---|---|
| What it is | a folder: `plugin.ini` and data files | the same folder, plus its code per device |
| Files to submit | the folder as a ZIP | the folder as a ZIP (without `code.ppn`), and one signed `.ppn` per device (`x4`, `x4-pro`) |
| Source | optional (`source` is still required: where it lives) | required and readable: a link to a fixed commit or tag, or a source ZIP |
| Signed | no | yes, every `.ppn`, with the author's own Ed25519 key |
| Checked | the ZIP's contents | the ZIP's contents, every `.ppn` (chip, fingerprint, digest, signature), and a rebuild from the source |

Readers see on the website and on the device whether a plugin has code,
who made it, its licence, its source, and for code plugins whether the
library's rebuild matched: **Verified: matches its source** or **Not
rebuilt**. A code plugin that is not rebuilt can still be listed; the
badge says so. A public version log lists every version published.

## 2. The two ways in

Both run the same checker; neither is quicker to the listing.

1. **The upload form on paperpaw.org** (the main way). The author signs
   in with a link sent by e-mail, uploads the files, sees the instant
   checks, then a preview of the plugin's page, and submits it into the
   moderation queue. A moderator approves it; then it is listed.
2. **A pull request** to the paperpaw.org repository adding (or, for an
   update, changing) `catalogue/<id>.toml`, with the files hosted
   elsewhere (`url` and `sha256`). The repository's CI runs the checker
   on the pull request; a maintainer merges it.

Ask the user which way they want. Without a preference, use the form.

## 3. Gather what the listing needs

Ask the user for each of these, and suggest values from the plugin
where you can. Section 6 has the exact rules.

- **id**: the plugin's id. Lower-case letters, digits and hyphens, at
  most 40, not starting or ending with a hyphen. It must be the same in
  `plugin.ini` (`id`), in the code plugin's manifest, and in the
  catalogue (`id` and the file name). It cannot change later: choose it
  now. It must not be taken (look at
  `https://paperpaw.org/api/v1/plugins.json`, the `id` of each plugin).
- **name**, **summary** (one sentence, at most 160 characters),
  **description** (optional, longer).
- **group**: `reading`, `formats`, `library`, `sync`, `home`, `look` or
  `system`.
- **tags**: up to five, lower-case letters, digits and hyphens, at most
  24 each.
- **devices**: for a code plugin, those it has builds for (`x4`,
  `x4-pro`); for a data plugin leave it out (every device) unless it
  only makes sense on some (`android`, `x4`, `x4-pro`, `x4-classic`).
- **licence**: an SPDX identifier (`Apache-2.0`, `MIT`, `GPL-3.0-only`,
  ...). The plugin must be the user's to publish under it.
- **author** (a person or a project), optional **author_url**.
- **source**: an `https://` address of the source anyone can open.
- **translations** of name and summary, optional (`[i18n.de]` and so on;
  English is the name and summary themselves).
- **screenshots**: optional, see section 6.

## 4. Prepare the plugin's folder

The folder is what a device installs into `plugins/<id>/`.

```text
my-plugin/
  plugin.ini        required, at the top
  i18n/en.ini       translations, settings.ini, keymap.ini, styles.css,
  i18n/de.ini       presets.ini, fonts/*.ttf|otf|ttc|ppfp, *.png, *.txt, *.md
```

`plugin.ini`:

```ini
[plugin]
id = my-plugin
name = My plugin
description = One sentence on what it does.
version = 1.0.0
formats = 1
```

Rules the checker applies to the folder (and to the ZIP made of it):

- `plugin.ini` lies at the top of the ZIP, not in a folder inside it.
- Its `id` is the catalogue's `id`, and its `version` is exactly the
  version being published (`1.0.0`, three numbers).
- `formats` is a number, 1 or more (1 when left out).
- Only these file types: `.ini`, `.css`, `.ttf`, `.otf`, `.ttc`,
  `.ppfp`, `.txt`, `.md`, `.png`. No code, no `code.ppn` (a code
  plugin's builds are submitted apart).
- At most 64 files (63 for a code plugin, whose `code.ppn` is the
  64th). Every path is parts of `A-Z a-z 0-9 . _ -` joined by `/`, none
  starting with a dot, at most 100 bytes.
- `i18n/*.ini` are UTF-8 and parse as translation catalogs; `plugin.ini`
  is UTF-8.
- The ZIP is at most 20 MB, its contents at most 40 MB unpacked.

Try the plugin before publishing it: in the emulator (copy the folder
into the library's `plugins`), and for a code plugin on a device if the
user has one (`docs/plugins.md`, "Trying it on a device").

## 5a. A data plugin: make the ZIP

The same command on Linux, macOS and Windows (Python 3), from the folder
that holds `my-plugin/`:

```sh
python3 -c "import shutil,sys; shutil.make_archive(sys.argv[1], 'zip', sys.argv[2])" my-plugin-1.0.0 my-plugin
python3 -m zipfile -l my-plugin-1.0.0.zip
```

(On Windows, `python` instead of `python3`.) Do not use PowerShell's
`Compress-Archive`: Windows PowerShell 5.1 writes `\` into the names,
which the checker refuses. Remove hidden files (`.DS_Store`, `.git`)
from the folder first.

Expected listing: `plugin.ini` at the top, other files as
`i18n/de.ini` and so on, nothing starting with `my-plugin/`:

```text
File Name                                             Modified             Size
plugin.ini                                     2026-10-20 12:00:00          164
i18n/                                          2026-10-20 12:00:00            0
i18n/de.ini                                    2026-10-20 12:00:00           95
```

For the pull request way, the ZIP must be downloadable over https (a
release asset of the plugin's repository, for example) and its SHA-256
goes into the catalogue:

```sh
sha256sum my-plugin-1.0.0.zip                  # Linux
shasum -a 256 my-plugin-1.0.0.zip              # macOS
(Get-FileHash my-plugin-1.0.0.zip).Hash.ToLower()   # Windows PowerShell
```

Then go on with section 6.

## 5b. A code plugin: build, sign, check

### The repository

Start from `samples/hello-native` in the PaperPaw repository (copy its
files into the plugin's own repository):

| File | What it is for |
|---|---|
| `Cargo.toml`, `src/main.rs` | the crate that makes the file: `paperpaw_sdk::plugin!(...)` around the plugin |
| `.cargo/config.toml` | how a plugin is linked, for both chips; keep it as it is |
| `rust-toolchain.toml` | the X4's compiler, the one the PaperPaw release names |
| `Cargo.lock` | committed: the exact versions of every crate, needed to rebuild |
| `plugin/plugin.ini` (and the rest of the folder) | the plugin's folder, without code |
| `build.sh` | builds both devices, packs, signs, zips the folder, writes `dist/` |
| `.github/workflows/build.yml` | runs `build.sh` on every tag and makes a release with the files |
| `.gitignore` | keeps `target/`, `dist/` and `*.key` out of the repository |
| `catalogue.toml` | the catalogue entry to fill in (section 6) |

In `build.sh` set `bin=` to the binary crate's name (`[[bin]] name` in
`Cargo.toml`). The plugin's own code stays a library crate, tested on a
computer and tried in the emulator as any other.

### The key (once per author, never again)

```sh
mkdir -p ~/.paperpaw
paperpaw-sign key ~/.paperpaw/author.key
```

Expected output (other digits):

```text
public key:  3f1c...64 hexadecimal digits...9a0b
fingerprint: 3F1C 22A0 9B7D 0E15
```

- The **public key** (64 digits) is what the catalogue's `author_key`
  and the form take. It may be shown anywhere.
- The **file** holds the secret key. Keep it outside every repository,
  back it up where the user keeps passwords (a password manager, an
  encrypted drive), and never lose it: devices remember the public key
  at the first install, and every update signed with another key warns
  every reader who has the plugin.
- `paperpaw-sign key` never overwrites a file (`error: ... exists: a key
  is never overwritten`). An author with a key already uses it for every
  plugin and every update; never make a second one by mistake.
- For CI, the file's contents become the repository's secret
  `PAPERPAW_PLUGIN_KEY`. Let the user do it in the repository's
  *Settings > Secrets and variables > Actions*, or, with their yes, run
  (it reads the file without showing it):

  ```sh
  gh secret set PAPERPAW_PLUGIN_KEY < ~/.paperpaw/author.key
  ```

- Signing in the browser on the upload form: **to be confirmed**
  (whether the form offers it, and that the key never leaves the page).

### Build for the right PaperPaw

Until PaperPaw 1.0 a `.ppn` runs only on firmware built by the same
compiler for the same chip from the same SDK. Its **fingerprint** (16
hexadecimal digits, the catalogue's `abi`) says which; a device loads
only files with its own, and *About* on the device shows its
fingerprint and compiler.

- Depend on the SDK of a released PaperPaw (`paperpaw-sdk = "0.5"` in
  `Cargo.toml`, the release's version), never on a development version
  or a path: a development SDK's fingerprint includes its source and
  matches no released firmware.
- Pin that release's compilers: `rust-toolchain.toml` `channel` = the
  release's `shells/xteink/rust-toolchain.toml`; the X4 Pro's espup
  toolchain = the release's `shells/xteink/versions.env`
  (`ESP_RUST_VERSION`, `ESP_GCC_VERSION`). The workflow's `PAPERPAW`,
  `RUST_X4`, `RUST_X4_PRO` and `ESP_GCC` say the same.
- Install `paperpaw-sign` of that release (its tag, here `v0.5.0`):

  ```sh
  cargo install --locked --git https://github.com/hthoene/paperpaw --tag v0.5.0 paperpaw-sign
  ```

- After every PaperPaw release, build again with its SDK and compilers
  and publish a new version (section 8); until then the store shows the
  plugin to nobody on the new firmware.

### Reproducible builds

The library rebuilds the source and compares. The build must give the
same bytes on any computer, in any folder. `build.sh` does what it
takes; keep to these rules so it holds:

- Build on **Linux** (the CI's `ubuntu-latest`, or WSL or a Linux
  container on Windows): paths in the code are written with `/`.
- **Commit `Cargo.lock`** and build with `--locked` (`build.sh` does).
- **The pinned toolchains** above, never `stable` or `nightly`.
- **Paths remapped**: `build.sh` adds `--remap-path-prefix` for the
  repository (`/plugin`), Cargo's home (`/cargo`) and, on the X4 Pro,
  the toolchain's `core` and `alloc` (`/rust`). Never set `RUSTFLAGS`:
  it replaces the flags in `.cargo/config.toml` that link the plugin.
- **Nothing from the moment or the computer** in the code: no build
  script that writes the date, the time, the user's name, a random
  number, a git hash of a dirty tree, or files from outside the
  repository; no `env!` of such variables.
- **Dependencies by version or fixed revision**: crates.io versions, or
  git dependencies with `rev = "<full commit hash>"`, never a branch.
- **Publish the commit you built**: tag it (`v1.0.0`) and push the tag;
  never build from uncommitted changes.

What the library runs to rebuild exactly (its commands, paths and the
toolchain it picks for a fingerprint) is **to be confirmed**; the rules
above are what it needs either way.

Test it before publishing: build twice, in two folders with two Cargo
homes, and compare.

```sh
git clone <repository> /tmp/first && git clone <repository> /tmp/elsewhere/second
. ~/export-esp.sh
(cd /tmp/first && ./build.sh)
(cd /tmp/elsewhere/second && CARGO_HOME=/tmp/cargo-second ./build.sh)
cmp /tmp/first/dist/my-plugin-1.0.0-x4.ppn /tmp/elsewhere/second/dist/my-plugin-1.0.0-x4.ppn
cmp /tmp/first/dist/my-plugin-1.0.0-x4-pro.ppn /tmp/elsewhere/second/dist/my-plugin-1.0.0-x4-pro.ppn
```

(With `CARGO_HOME` changed, put `paperpaw-sign` on the `PATH` from the
first Cargo home.) Expected: `cmp` prints nothing. If it prints
`differ`, compare `paperpaw-sign plugin` of both: a different
`fingerprint:` means another compiler or SDK; the same fingerprint with
another `digest:` means paths or something of the moment got into the
code (section 9). The library's own check from the command line is
**to be confirmed**.

### Build, pack and sign

Locally (Linux, both toolchains installed):

```sh
. ~/export-esp.sh
./build.sh --key ~/.paperpaw/author.key
```

Or in CI: commit, then tag and push with the user's yes:

```sh
git tag v1.0.0
git push origin v1.0.0
```

The workflow fails at once if the tag is not `v` + `plugin.ini`'s
version. It attaches to the tag's GitHub release, and to the job's
artifact `plugin`:

```text
dist/my-plugin-1.0.0.zip            the folder, for the catalogue's url and sha256
dist/my-plugin-1.0.0-x4.ppn         the X4's file, signed
dist/my-plugin-1.0.0-x4-pro.ppn     the X4 Pro's file, signed
dist/SHA256SUMS                     sha256 of each
dist/catalogue-versions.toml        the [[versions]] block for the catalogue
```

`build.sh` ends by printing each file's fingerprint and the sums:

```text
x4: fingerprint 85073a61323717ff
x4-pro: fingerprint 1c0ffee2a5b3d4e6
```

### Check each file

```sh
paperpaw-sign check-plugin dist/my-plugin-1.0.0-x4.ppn
paperpaw-sign plugin dist/my-plugin-1.0.0-x4.ppn
```

Expected (`check-plugin`; digits differ):

```text
plugin:      61892 bytes for the ESP32-C3, fingerprint 85073a61323717ff
signer:      3F1C 22A0 9B7D 0E15 (3f1c...9a0b)
signature:   holds (...)
```

Check, for each file:

- `chip:` is `ESP32-C3` for `x4`, `ESP32-S3` for `x4-pro`;
- `fingerprint:` is the one *About* shows on a device with that release;
- the 64 digits in brackets after `signer:` are the author's public key
  (the same in every file, every version);
- `signature:   holds`. `signer:      none, not signed` means the file
  was packed without `--key`: sign it (`paperpaw-sign sign-plugin
  <file> --key <path>`) or build again with the key.

## 6. The catalogue entry

The form asks for these; the pull request way writes them into
`catalogue/<id>.toml`. A field not in this table is refused.

| Field | Required | Rule |
|---|---|---|
| `id` | yes | as in section 3; the file is `catalogue/<id>.toml` |
| `name` | yes | not empty |
| `summary` | yes | one sentence, at most 160 characters |
| `description` | no | longer text for the plugin's page |
| `kind` | yes | `"data"` or `"code"` |
| `author` | yes | a person or project |
| `author_url` | no | the author's page |
| `author_key` | code plugins with versions | the public key, 64 lower-case hexadecimal digits; refused on data plugins |
| `licence` | yes | an SPDX identifier |
| `source` | yes | an `https://` address |
| `crate` | code plugins | the plugin's library crate (`name` in its `Cargo.toml`) |
| `group` | yes | one of the seven groups |
| `tags` | no | up to 5, `a-z 0-9 -`, at most 24 each, none twice |
| `devices` | code plugins with versions | `android`, `x4`, `x4-pro`, `x4-classic`; every build's device must be here; leave out for "every device" (data plugins) |
| `screenshots` | no | up to 4 tables `{ file = "<id-like-name>.png", alt = "what it shows" }`; `.png`, `.jpg` or `.webp`, at most 500 KB, at most 200 characters of `alt`; for a pull request in `catalogue/screenshots/<id>/` |
| `[i18n.<lang>]` | no | `name` and `summary` in that language (two or three lower-case letters, not `en`) |
| `[[versions]]` | yes (one or more) | see below |

Each `[[versions]]`:

| Field | Rule |
|---|---|
| `version` | three numbers, `1.0.0`; equal to `plugin.ini`'s `version` |
| `sdk` | the SDK it is made for, `0.5` |
| `date` | `YYYY-MM-DD` (optional) |
| `url` and `sha256` | the folder's ZIP over https, and its SHA-256 (64 hexadecimal digits) |
| `folder` | only for folders inside the paperpaw.org repository (PaperPaw's own samples); authors use `url` |

Each `[[versions.builds]]` (code plugins, one per device, at least one):

| Field | Rule |
|---|---|
| `device` | `x4` or `x4-pro`; also listed in `devices`; no two for one device |
| `abi` | the file's fingerprint, 16 lower-case hexadecimal digits |
| `url` and `sha256` | the `.ppn` over https, and the SHA-256 of the whole file (with its signature) |

A data plugin, complete:

```toml
id = "my-theme"
name = "My theme"
summary = "Dark headers and square corners."
kind = "data"
author = "Jane Doe"
licence = "Apache-2.0"
source = "https://github.com/jane/my-theme"
group = "look"
tags = ["theme"]

[i18n.de]
name = "Mein Thema"
summary = "Dunkle Kopfzeilen und eckige Ecken."

[[versions]]
version = "1.0.0"
sdk = "0.5"
date = "2026-10-20"
url = "https://github.com/jane/my-theme/releases/download/v1.0.0/my-theme-1.0.0.zip"
sha256 = "<64 hexadecimal digits>"
```

A code plugin: `samples/hello-native/catalogue.toml`, its
`[[versions]]` from the release's `catalogue-versions.toml`.

**To be confirmed**: where a code plugin's fixed commit, tag or source
ZIP for each version goes (today only `source`, one address for the
plugin); whether `x4-classic` takes the X4 Pro's file as a build of its
own.

## 7a. Submit with the upload form

1. Open the form: address **to be confirmed** (on paperpaw.org).
2. The user signs in with their e-mail address and opens the link sent
   to them. The assistant never handles that mail or link.
3. Enter the fields of section 6 (the form's field names are **to be
   confirmed**; match them to section 6 and ask where unsure).
4. Upload: a data plugin's ZIP; or a code plugin's folder ZIP and its
   `.ppn` files, one per device, and its source (a link to the tagged
   commit, e.g. `https://github.com/jane/my-plugin/tree/v1.0.0`, or a
   source ZIP: `git archive --format=zip -o my-plugin-1.0.0-source.zip
   v1.0.0`).
5. Read the instant checks. Each problem is one line; fix it (section 9)
   and upload again.
6. Look at the preview of the plugin's page with the user.
7. Submit to the moderation queue only after the user says so.

## 7b. Submit by pull request

With the user's yes for each step:

```sh
gh repo fork hthoene/paperpaw.org --clone
cd paperpaw.org
git switch -c add-my-plugin
# write catalogue/my-plugin.toml (section 6); screenshots into catalogue/screenshots/my-plugin/
git add catalogue/my-plugin.toml
git commit -m "catalogue: add my-plugin 1.0.0"
git push -u origin add-my-plugin
gh pr create --title "catalogue: add my-plugin 1.0.0" --body "A new plugin: <one sentence>. Source: <link to the tag>."
```

The repository's address and branch to target are **to be confirmed**
(the website's public repository). The pull request's checks run the
same checker as the form; read their output and fix what they name.

## 8. Updates

An update is a new version through the same way:

1. Raise the version everywhere: `plugin.ini` `version`, the plugin's
   manifest, `Cargo.toml` `version` (code plugins). The version must be
   higher than every one published.
2. Code plugins: build, sign **with the same key**, check (section 5b).
   A new PaperPaw release needs a new version too, built with its SDK
   and compilers (a new `abi`).
3. Submit: in the form, as a new version of the plugin (**to be
   confirmed** how the form names it); by pull request, add a
   `[[versions]]` block to the existing entry and keep the older ones.
4. The version log on paperpaw.org lists it once it is published.

## 9. Messages and how to fix them

### paperpaw-sign

Each starts with `error: `.

| Message (or its end) | Cause | Fix |
|---|---|---|
| `... exists: a key is never overwritten` | a key file is there | use that key; never delete it to make another |
| `the key is not 64 hexadecimal digits` | wrong file passed to `--key` | pass the key file `paperpaw-sign key` made |
| `no --key and no PAPERPAW_SIGNING_KEY` | `sign-plugin` without a key | add `--key <path>` |
| `an object, not a linked plugin: pack the plugin as linked` | an `.o`/`.rlib` was passed | pass `target/<target>/release/<bin>` |
| `made for machine N, neither RISC-V nor Xtensa` | built for the computer | build with `--target` (`build.sh` does) |
| `no symbol table: do not strip the plugin` | `strip` in the profile | remove `strip` from `[profile.release]` |
| `... : link the plugin with paperpaw-plugin.x` | `.cargo/config.toml` missing or changed, or `RUSTFLAGS` set | restore the sample's `.cargo/config.toml`; unset `RUSTFLAGS` |
| ``no relocations kept: link the plugin with `--emit-relocs` `` | as above | as above |
| ``... : a plugin's entry is made by the SDK's `plugin!` `` | no `plugin!` in the binary crate | `paperpaw_sdk::plugin!(my_plugin::MyPlugin::default());` in `src/main.rs` |
| `a plugin has no writable data (its state lives on the kernel's heap), but this one has: ...` | a `static mut`, `static` with interior mutability, or a crate that keeps one | keep state on the heap (fields of the plugin, `Rc` given to closures); drop the crate named |
| `an unexpected section X (n bytes): a plugin is only its .rodata and .text` | thread locals, `#[link_section]`, a crate with its own sections | remove what makes the section |
| `the image is N bytes, more than the M a plugin may have` | too large (4 MiB) | `docs/plugins.md`, "Keeping it small" |
| ``the plugin cannot be packed (...): undefined symbol `x` (used at ...)`` | it calls something neither it nor the SDK brings (e.g. `std`, C code, float functions from libm) | remove the call or bring the code in Rust |
| `... : not known to this tool` / `... points outside the image` | code that reaches a fixed address | do not use addresses of the firmware or ROM; `docs/plugins.md`, "Packing and signing" |
| `<file>: not signed` (`check-plugin`) | packed without `--key` | `paperpaw-sign sign-plugin <file> --key <path>` |
| `<file>: changed or damaged` | the file changed after packing | build and pack again; never edit it |
| `<file>: its author's signature does not hold` | the file or its trailer changed | sign again with the author's key |
| `<file>: not a plugin file` / `not as long as it says` | not a `.ppn`, or cut short in a download | use the file `pack` wrote |

### The library's checks (form and pull request)

Messages name the plugin, e.g. `my-plugin: ...`, and the version,
`my-plugin: version 1.0.0: ...`.

| Message (its end) | Fix |
|---|---|
| ``<id>.toml: the id is `x`, not the file's name`` | name the file `catalogue/<id>.toml` |
| `an id is lower-case letters, digits and hyphens, at most 40` | choose a valid id, the same everywhere |
| `a name and a summary are needed` | fill both in |
| `the summary is one sentence, at most 160 characters` | shorten it |
| `group is one of reading, formats, library, sync, home, look, system` | pick one of those |
| `at most 5 tags` / ``tag `x`: lower-case letters, digits and hyphens, at most 24`` / ``tag `x` twice`` | fix the tags |
| `i18n.xx: ...` | language as two or three lower-case letters, not `en`; both name and summary |
| `screenshot x: ...` | see the screenshot rules in section 6 |
| `source is an https address` | an `https://` link |
| `only a plugin with builds names an author_key` | remove `author_key` from a data plugin |
| `a plugin with code names its crate` | add `crate` |
| `a plugin with builds names the devices it runs on` | add `devices = ["x4", "x4-pro"]` |
| `a plugin with builds names its author's key` / `author_key is 64 lower-case hexadecimal digits` | the public key from `paperpaw-sign key` or `check-plugin`, lower case |
| `a data plugin has at least one version` | add a `[[versions]]` |
| `version x: a version is like 1.2.0` / ``sdk `x` is not like 0.3`` | fix the numbers |
| `either a folder, or a url and its sha256` | give `url` and `sha256` together |
| `a url is an https address` / `sha256 is 64 hexadecimal digits` | fix them |
| `a data plugin has no builds` / `a plugin with code has at least one build` | match `kind` to what is submitted |
| `a build for x, which is not in its devices` / `two builds for x` / `no builds for android yet` | list each device once, in `devices` too; no Android builds yet |
| `x: abi is 16 lower-case hexadecimal digits` | the `fingerprint:` from `paperpaw-sign plugin` |
| `its SHA-256 is A, the catalogue says B` | the file at `url` is not the one summed: sum the file published, never edit the hash to fit another file |
| `not a ZIP: ...` / `larger than 20 MB` / `more than 40 MB unpacked` | make the ZIP as in section 5a |
| `x: a name outside the folder` / `x: names are letters, digits, dots, hyphens and underscores, at most 100 bytes` | rename the file; zip as in section 5a |
| `more than 64 files` (63 with code) / `x: twice` | fewer files |
| `x: only ini, css, ttf, otf, ttc, ppfp, txt, md, png files belong in a data plugin` | remove that file (a `.ppn` is submitted apart) |
| `no plugin.ini at the top` | zip the folder's contents, not the folder |
| `plugin.ini names id ..., the catalogue x` / `plugin.ini names version ..., the catalogue x` | make them equal |
| ``plugin.ini: formats `x` is not a number such as 1`` | `formats = 1` |
| `built for chip 5, x4-pro has chip 9` (or the other way) | the files of the two devices are swapped |
| `its fingerprint is A, the catalogue says B` | `abi` from that very file |
| `not signed by its author` | sign it (section 5b) |
| `N bytes, its header says M with its author's signature` | the file was cut or had bytes added: use the file `pack` wrote |
| `changed or damaged: its digest does not hold` | build and pack again |
| `signed by the key A, the catalogue names B` | sign with the author's key; if the key really changed, section 10 |
| `its author's signature does not hold` / `its author's signature is not one` | sign again with `sign-plugin` |
| `larger than 4 MB` | make it smaller |

The rebuild's messages, and what makes a plugin **Not rebuilt**, are
**to be confirmed**. When it says so, run the two-folder test in
section 5b and fix what differs.

## 10. Changing or losing the key

- **Lost** (no copy anywhere): make a new key, put its public key into
  the catalogue entry's `author_key` (through the form or a pull
  request, in the open), and publish a new version signed with it.
  Every reader who has the plugin is warned at that update and decides.
- **Leaked** (shown in a chat, committed, in a log): the same as lost,
  at once, and remove the old secret from the CI (`PAPERPAW_PLUGIN_KEY`)
  and from the repository's history. Tell the user plainly.
- Never change keys for convenience: the warning is what protects
  readers from a stranger's code.

## 11. Final checklist

Before the user submits, every line must hold:

- [ ] The id is valid, free, and the same in `plugin.ini`, the code, the
      catalogue's `id` and file name.
- [ ] `plugin.ini`'s `version` is the version submitted, higher than
      every published one, and (code) the git tag is `v` + that version.
- [ ] The ZIP lists `plugin.ini` at the top, only allowed file types, at
      most 64 files (63 with code), no hidden files, no `code.ppn`.
- [ ] Every catalogue field is from section 6, filled in with real
      values (no `YOUR ...` left from the sample).
- [ ] Code: `Cargo.lock` committed; built from the tagged commit, on
      Linux, with the release's SDK and pinned compilers.
- [ ] Code: the two-folder build gave the same files (`cmp` silent).
- [ ] Code: `paperpaw-sign check-plugin` says `signature:   holds` for
      every `.ppn`; the chips are ESP32-C3 (x4) and ESP32-S3 (x4-pro);
      the key in brackets is the catalogue's `author_key`.
- [ ] Code: each build's `abi` is its file's fingerprint, each
      `sha256` its file's, each `url` serves that file.
- [ ] The source is public at the tagged commit (or the source ZIP is
      ready), under the licence named.
- [ ] No secret anywhere: no `*.key` in the repository or its history,
      the key in no file, chat or log; `PAPERPAW_PLUGIN_KEY` only as a
      CI secret.
- [ ] Tried in the emulator; code plugins on a device if the user has
      one.
- [ ] The user has seen the entry and the files and said yes to
      submitting.

## 12. To be confirmed (website not settled yet)

- The upload form's address, its field names, the sign-in page, and how
  an update is submitted there.
- Signing in the browser on the form.
- Where a version's fixed commit, tag or source ZIP goes in the
  catalogue.
- The rebuild: the commands, paths and toolchain the library uses for a
  fingerprint, its messages, and a check authors can run themselves.
- Builds for the `x4-classic` (the X4 Pro's chip).
- The paperpaw.org repository and branch that pull requests go to.
