//! Addresses ([`Url::parse`]), the requests built for them ([`request`])
//! and where redirects lead ([`redirect`]): odd hosts and ports, IPv6
//! literals, credentials, fragments, header injection through names,
//! values and addresses, and relative and absolute `Location`s.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

use paperpaw_http::server::parse_request;
use paperpaw_http::{Url, redirect, request};

fn url(text: &str) -> Url<'_> {
    Url::parse(text).unwrap()
}

#[test]
fn schemes_other_than_http_are_refused() {
    for bad in [
        "",
        "http:",
        "http:/host",
        "https:/",
        "ftp://host/",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "//host/path",
        "/relative",
        "htt",
        "\u{e4}\u{f6}\u{fc}",
    ] {
        assert_eq!(Url::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn hosts_must_not_be_empty() {
    for bad in [
        "http://",
        "https://",
        "http:///path",
        "http://:80/",
        "http://user@/",
    ] {
        assert_eq!(Url::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn ports_must_be_numbers_that_fit() {
    assert_eq!(url("http://h:65535/").port, 65535);
    // An empty port is the scheme's own (RFC 3986).
    assert_eq!(url("http://h:/").port, 80);
    assert_eq!(url("https://h:/").port, 443);
    for bad in [
        "http://h:65536/",
        "http://h:-1/",
        "http://h:+1/",
        "http://h:8o/",
        "http://h:0/",
    ] {
        assert_eq!(Url::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn default_ports_follow_the_scheme() {
    assert_eq!(url("http://h/").port, 80);
    assert_eq!(url("https://h/").port, 443);
    assert_eq!(url("https://h:443/").host_header(), "h");
    assert_eq!(url("http://h:443/").host_header(), "h:443");
    assert_eq!(url("https://h:80/").host_header(), "h:80");
}

#[test]
fn credentials_are_dropped_up_to_the_last_at() {
    let parsed = url("http://user:p@ss@host.org:81/p");
    assert_eq!((parsed.host, parsed.port), ("host.org", 81));
    // An `@` after the authority belongs to the path.
    let parsed = url("http://host.org/mail@example");
    assert_eq!((parsed.host, parsed.path), ("host.org", "/mail@example"));
}

#[test]
fn fragments_are_never_sent() {
    assert_eq!(url("http://h/a#b/c?d").path, "/a");
    assert_eq!(url("http://h#frag").path, "/");
    assert_eq!(url("http://h/a?q=1#x").path, "/a?q=1");
}

/// The host comes without its brackets; the `Host` header puts them back.
#[test]
fn ipv6_literals_with_a_port() {
    let parsed = url("http://[::1]:8080/x");
    assert_eq!((parsed.host, parsed.port), ("::1", 8080));
    assert_eq!(parsed.host_header(), "[::1]:8080");
}

#[test]
fn ipv6_literals_without_a_port() {
    let parsed = url("http://[fe80::1]/x");
    assert_eq!((parsed.host, parsed.port), ("fe80::1", 80));
    assert_eq!(parsed.host_header(), "[fe80::1]");
}

#[test]
fn built_requests_read_back_on_the_server_side() {
    let target = url("http://h.org:8080/api/list?d=books&x=%20");
    let head = request("PUT", &target, &[], Some(12));
    let (read, used) = parse_request(&head).unwrap().unwrap();
    assert_eq!(used, head.len());
    assert_eq!(read.method, "PUT");
    assert_eq!(read.target, "/api/list?d=books&x=%20");
    assert_eq!(read.length, 12);
    assert!(!read.chunked);
    let text = String::from_utf8(head).unwrap();
    assert!(text.contains("\r\nHost: h.org:8080\r\n"));
}

#[test]
fn our_own_headers_cannot_be_overridden_in_any_case() {
    let headers = [
        (String::from("HOST"), String::from("evil")),
        (String::from("connection"), String::from("keep-alive")),
        (String::from("Content-LENGTH"), String::from("999")),
        (String::from("user-agent"), String::from("Mine/1")),
    ];
    let text = String::from_utf8(request("GET", &url("http://h/"), &headers, None)).unwrap();
    assert_eq!(
        text,
        "GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\nuser-agent: Mine/1\r\n\r\n"
    );
}

#[test]
fn header_names_and_values_with_line_breaks_are_left_out() {
    let headers = [
        (String::from("X-A\nX-Evil"), String::from("1")),
        (String::from("X-B"), String::from("1\rX-Evil: 2")),
        (String::from("X-C"), String::from("ok")),
    ];
    let text = String::from_utf8(request("GET", &url("http://h/"), &headers, None)).unwrap();
    assert!(!text.contains("Evil"), "{text}");
    assert!(text.contains("\r\nX-C: ok\r\n"));
}

#[test]
fn line_breaks_in_an_address_inject_no_headers() {
    for address in [
        "http://h/a\r\nX-Injected: 1\r\n",
        "http://h/a\nX-Injected: 1",
        "http://h\r\nX-Injected: 1\r\n/",
    ] {
        let Some(parsed) = Url::parse(address) else {
            continue; // refusing such an address is fine too
        };
        let text = String::from_utf8(request("GET", &parsed, &[], None)).unwrap();
        assert!(!text.contains("\nX-Injected"), "{address:?} gave {text:?}");
    }
}

#[test]
fn absolute_locations_replace_the_address() {
    let base = "https://h.org/a/b";
    assert_eq!(redirect(base, "http://o.org").unwrap(), "http://o.org");
    assert_eq!(
        redirect(base, "  HTTPS://O.org/x?y  ").unwrap(),
        "HTTPS://O.org/x?y"
    );
}

#[test]
fn relative_locations_keep_scheme_host_and_port() {
    let base = "https://h.org:8443/a/b/c?q=1";
    assert_eq!(redirect(base, "/x").unwrap(), "https://h.org:8443/x");
    assert_eq!(redirect(base, "x").unwrap(), "https://h.org:8443/a/b/x");
    assert_eq!(
        redirect(base, "//cdn.org/y").unwrap(),
        "https://cdn.org/y",
        "a network-path reference keeps only the scheme"
    );
    assert_eq!(redirect("http://h.org", "x").unwrap(), "http://h.org/x");
}

#[test]
fn redirects_from_unusable_bases_are_none() {
    assert_eq!(redirect("not a url", "/x"), None);
    assert_eq!(redirect("ftp://h/", "x"), None);
}

#[test]
fn relative_locations_from_a_base_without_a_path() {
    assert_eq!(
        redirect("http://h.org?page=1", "login").unwrap(),
        "http://h.org/login"
    );
}

#[test]
fn query_only_locations_keep_the_path() {
    // RFC 3986, 5.4.1: "?y" against http://a/b/c/d;p?q is http://a/b/c/d;p?y
    assert_eq!(
        redirect("http://h.org/a/b?x", "?y").unwrap(),
        "http://h.org/a/b?y"
    );
}
