//! Links as text: what Add accepts ([`parse_link`]) and the magnet link a torrent is copied as
//! ([`magnet_link`]). The two agree: parsing a torrent's magnet link gives back its info hash,
//! name and trackers.

use proto::TorrentInfo;

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
            actions: Vec::new(),
        }
    }

    proptest! {
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
