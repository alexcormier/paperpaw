# 0017. TLS on the X4

- Status: Accepted
- Date: 2026-10-18

## Context

Nearly every service plugins talk to (catalogs, sync, news, updates)
needs HTTPS. Android and desktops have the system's TLS; the X4 has
nothing ([ADR 0016](0016-network-and-wifi.md) brought plain HTTP). Its
limits:

- About 100 KB of heap free with the radio on, in pieces (the largest
  block about 64 KB).
- The ESP32-C3 has no atomic read-modify-write, so `alloc::sync::Arc`
  is not available: rustls cannot be used.
- No clock: the X4 does not know the date until it gets the time over the
  network.
- The firmware polls from one main loop and must never block.

Writing TLS ourselves would save little and risk much: the largest cost,
a 16 KB buffer for incoming records, is the protocol's (a server may send
records that long). The pure Rust option for small devices, embedded-tls
(TLS 1.3 only), had open faults in certificate verification at the time
(a handshake accepted without server authentication; chain constraints
not checked).

## Decision

- **mbedTLS 3** (Espressif's port, through `mbedtls-rs` and its raw
  bindings, prebuilt for `riscv32imc-unknown-none-elf`: no C toolchain
  needed): TLS 1.2 and 1.3, server name indication, the host checked
  against the certificate.
- **A session of our own over the raw bindings** (`shells/xteink/src/tls.rs`),
  not `mbedtls-rs`'s: it works on buffers (encrypted bytes in and out,
  moved to and from the TCP socket by the main loop, never blocking),
  and it installs its own verify callback. `mbedtls-rs` is kept for its
  global set-up (the random numbers for PSA).
- **Mozilla's root certificates stay in flash** (`webpki-roots`: each
  root's name and key, about 50 KB) and are never parsed into memory, as a
  list of trusted certificates for mbedTLS would be (well over 100 KB).
  mbedTLS is given an empty list, so it finds the top of every chain
  untrusted; the verify callback (`shells/xteink/src/roots.rs`) then
  trusts it if it is one of the roots itself (name and key; a root sent
  along, maybe cross-signed by another, as `example.com`'s is) or is
  signed by one (found by name, the signature checked with its key). This
  is the approach of ESP-IDF's certificate bundle. Every other finding of
  mbedTLS stays (chain signatures, constraints, the host).
- **One TLS connection at a time**: a session takes about 40 KB (two
  record buffers of 16 KB, the handshake). Further HTTPS requests wait for
  it; plain ones do not.
- **No allocation of the network path may fail hard**: socket buffers,
  the session's state and bodies kept in memory are asked for fallibly,
  so that memory short or in pieces makes a request fail with a reason,
  not the X4 restart.
- **Software arithmetic for now**: mbedTLS's hooks for the chip's SHA,
  AES and RSA units work through esp-hal's work queues, which block for
  ever if nothing services them; that comes as a step of its own, tried
  carefully.

## Consequences

- HTTPS works on the X4: a handshake takes 1.3 to 2.7 s (ECDSA P-384 the
  slowest), a download about 34 KB/s (plain HTTP about 75 KB/s). Untrusted
  roots, self-signed certificates, incomplete chains and certificates for
  another host are refused with a reason for the plugin.
- **Not yet checked: certificates' dates.** Without a clock an expired or
  not yet valid certificate is accepted (the chain and the host are still
  checked). Once the X4 has the time over the network, the verify
  callback checks the dates of every certificate of the chain itself
  (`valid_from`, `valid_to`), with no rebuild of mbedTLS needed.
- Not checked: revocation (as in most clients without OCSP stapling),
  roots' name constraints (none of Mozilla's TLS roots has any).
- The firmware grows by about 400 KB of flash; the stack, above `.bss`,
  lost about 4 KB to mbedTLS's static data, and the handshake runs deep:
  after a first book open about 11 KB of the stack stay unused.
- Smaller record buffers (4 KB out) or hardware acceleration need either
  a rebuild of mbedTLS (clang, cmake, ninja) or careful hooking; both are
  measured improvements for later, not needed to work.
