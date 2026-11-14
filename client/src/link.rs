//! Links as text: what Add accepts ([`parse_link`]) and the magnet link a torrent is copied as
//! ([`magnet_link`]). The two agree: parsing a torrent's magnet link gives back its info hash,
//! name and trackers.

use proto::{AddSource, TorrentInfo};

use crate::LinkProblem;

/// What a link names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// A magnet link: the info hash as written, and the name and trackers it carries.
    Magnet { info_hash: String, name: Option<String>, trackers: Vec<String> },
    /// An info hash on its own.
    InfoHash(String),
    /// The http(s) URL of a .torrent file.
    Url(String),
}

/// Whether the text is something Add accepts: a magnet link with an info hash, an info hash on its
/// own, or an http(s) URL. An info hash is 40 hex digits or 32 base32 letters and digits.
pub fn parse_link(text: &str) -> Result<Link, LinkProblem> {
    let text = text.trim();
    if let Some(query) = text.strip_prefix("magnet:?") {
        let mut info_hash = None;
        let (mut name, mut trackers) = (None, Vec::new());
        for pair in query.split('&') {
            if let Some(hash) = pair.strip_prefix("xt=urn:btih:") {
                info_hash = info_hash.or(Some(hash));
            } else if let Some(dn) = pair.strip_prefix("dn=") {
                name = name.or_else(|| Some(percent_decode(dn)));
            } else if let Some(tr) = pair.strip_prefix("tr=") {
                trackers.push(percent_decode(tr));
            }
        }
        return match info_hash.filter(|h| is_info_hash(h)) {
            Some(hash) => Ok(Link::Magnet { info_hash: hash.to_owned(), name, trackers }),
            None => Err(LinkProblem::NoInfoHash),
        };
    }
    let url = ["http://", "https://"].iter().any(|scheme| text.strip_prefix(scheme).is_some_and(|rest| !rest.is_empty()));
    if url {
        Ok(Link::Url(text.to_owned()))
    } else if is_info_hash(text) {
        Ok(Link::InfoHash(text.to_owned()))
    } else {
        Err(LinkProblem::NotALink)
    }
}

/// What Add sends for `text`, by [`parse_link`]'s rules: a magnet link as it is, a bare info hash
/// as the magnet link of it, a .torrent URL to download.
pub fn add_source(text: &str) -> Result<AddSource, LinkProblem> {
    Ok(match parse_link(text)? {
        Link::Magnet { .. } => AddSource::Magnet(text.trim().to_owned()),
        Link::InfoHash(hash) => AddSource::Magnet(format!("magnet:?xt=urn:btih:{hash}")),
        Link::Url(url) => AddSource::TorrentUrl(url),
    })
}

fn is_info_hash(h: &str) -> bool {
    match h.len() {
        40 => h.bytes().all(|b| b.is_ascii_hexdigit()),
        32 => h.bytes().all(|b| matches!(b.to_ascii_uppercase(), b'A'..=b'Z' | b'2'..=b'7')),
        _ => false,
    }
}

/// The torrent's magnet link: its info hash, name and trackers. It works for a torrent added from
/// a .torrent file too.
pub fn magnet_link(torrent: &TorrentInfo) -> String {
    let mut link = format!("magnet:?xt=urn:btih:{}", torrent.info_hash);
    if let Some(name) = &torrent.name {
        link.push_str("&dn=");
        percent_encode(name, &mut link);
    }
    for tracker in &torrent.trackers {
        link.push_str("&tr=");
        percent_encode(tracker, &mut link);
    }
    link
}

fn percent_encode(text: &str, out: &mut String) {
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
}

/// `%XX` escapes, and `+` for a space, as magnet links in the wild have it.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match bytes[i] {
            b'%' if i + 2 < bytes.len() && hex(bytes[i + 1]).is_some() && hex(bytes[i + 2]).is_some() => {
                out.push((hex(bytes[i + 1]).unwrap() * 16 + hex(bytes[i + 2]).unwrap()) as u8);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use proto::Status;

    use super::*;

    fn torrent(info_hash: String, name: Option<String>, trackers: Vec<String>) -> TorrentInfo {
        TorrentInfo {
            id: 1,
            info_hash,
            name,
            status: Status::Downloading,
            content: None,
            uploaded_bytes: 0,
            download_bytes_per_sec: 0,
            upload_bytes_per_sec: 0,
            live_peers: 0,
            trackers,
            location: None,
            actions: proto::Actions::default(),
        }
    }

    /// Text that's nearly a link, or one: magnets with whole, short or no hashes, bare hashes of
    /// both kinds, URLs, and other words.
    fn texts() -> impl Strategy<Value = String> {
        prop_oneof![
            "[0-9a-f]{40}",
            "[A-Z2-7]{32}",
            "[0-9a-f]{39}",
            "magnet:\\?xt=urn:btih:[0-9a-f]{38,41}(&dn=[a-z]{1,8})?",
            "magnet:\\?dn=[a-z]{1,8}",
            "https?://[a-z]{0,8}(/[a-z]{1,8}\\.torrent)?",
            "ftp://[a-z]{1,8}",
            " ?\\PC{0,20} ?",
        ]
    }

    proptest! {
        /// An add's source is exactly what the field accepts: a magnet carries the info hash the
        /// text names (a bare one made a magnet link), and a .torrent URL is the URL.
        #[test]
        fn an_add_source_is_the_link_parsed(text in texts()) {
            match (parse_link(&text), add_source(&text)) {
                (Err(problem), source) => prop_assert_eq!(source, Err(problem)),
                (Ok(Link::Url(url)), source) => prop_assert_eq!(source, Ok(AddSource::TorrentUrl(url))),
                (Ok(link), Ok(AddSource::Magnet(magnet))) => {
                    let hash = |l: Link| match l {
                        Link::Magnet { info_hash, .. } | Link::InfoHash(info_hash) => info_hash,
                        Link::Url(_) => unreachable!("not a magnet"),
                    };
                    let parsed = parse_link(&magnet);
                    prop_assert!(matches!(parsed, Ok(Link::Magnet { .. })), "{magnet} is a magnet link");
                    prop_assert_eq!(hash(parsed.unwrap()), hash(link));
                }
                (Ok(link), source) => prop_assert!(false, "{link:?} sent as {source:?}"),
            }
        }

        /// The magnet link of any torrent parses back to its info hash, name and trackers.
        #[test]
        fn a_magnet_link_round_trips(
            hash in "[0-9a-f]{40}",
            name in proptest::option::of("\\PC{0,40}"),
            trackers in proptest::collection::vec("\\PC{1,40}", 0..4),
        ) {
            let link = magnet_link(&torrent(hash.clone(), name.clone(), trackers.clone()));
            prop_assert_eq!(parse_link(&link), Ok(Link::Magnet { info_hash: hash, name, trackers }), "{}", link);
        }
    }
}
