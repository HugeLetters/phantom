//! macOS integration: what the system hands Phantom from outside its window. Today that's the
//! magnet links clicked anywhere (the bundle declares the scheme).

use rune::Tx;

use crate::model::Phantom;
use crate::service::{AddRequest, Notice, Service};

/// Rune's `on_open_url`: macOS hands us a link; add it and bring the window back.
pub fn open_url(s: &mut Phantom, url: &str, _: &mut Tx) {
    match open_link(&mut *s.service, url) {
        Ok(()) => {
            if let Some(window) = &s.window {
                window.open();
            }
        }
        Err(notice) => s.notice = Some(notice),
    }
}

/// The URL comes from outside the app, so only magnets go through.
pub fn open_link(service: &mut dyn Service, url: &str) -> Result<(), Notice> {
    if !url.starts_with("magnet:") {
        return Err(Notice::error(format!("Phantom only opens magnet links, not {url}")));
    }
    service.add(AddRequest::Url(url.to_owned()));
    Ok(())
}
