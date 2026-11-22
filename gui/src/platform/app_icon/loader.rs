//! Reading the images added as icons off the main thread (#87). A big image takes a while to read
//! and decode, so neither the launch nor the sheet waits for it: the window opens at once, a half
//! that names an image shows the theme's icon until it's here, and the tile is a placeholder.
//!
//! A [`Loader`] is asked for an image by its id and answers by [`Loader::next`], on the thread
//! that asks, as each is ready.

#[cfg(test)]
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender, channel};

use super::{Icon, read_image};
use rune::Waker;

/// An image read: the id asked for, and the icon, or why it can't be one.
pub type Loaded = (String, Result<Icon, String>);

enum How {
    /// Read at once, on the thread that asks: for a test, or a headless run, which waits for it.
    Now,
    /// Kept until [`Loader::release`]: for a test of what shows meanwhile.
    #[cfg(test)]
    Held(RefCell<Vec<String>>),
    /// Each on a thread of its own; `wake` tells the window there's an answer.
    Background(Waker),
}

pub struct Loader {
    how: How,
    tx: Sender<Loaded>,
    rx: Receiver<Loaded>,
}

impl Loader {
    fn with(how: How) -> Self {
        let (tx, rx) = channel();
        Self { how, tx, rx }
    }

    pub fn now() -> Self {
        Self::with(How::Now)
    }

    #[cfg(test)]
    pub fn held() -> Self {
        Self::with(How::Held(RefCell::new(Vec::new())))
    }

    pub fn background(wake: Waker) -> Self {
        Self::with(How::Background(wake))
    }

    /// Reads the image `id` names.
    pub fn request(&self, id: String) {
        match &self.how {
            How::Now => {
                let _ = self.tx.send((id.clone(), read_image(&id)));
            }
            #[cfg(test)]
            How::Held(held) => held.borrow_mut().push(id),
            How::Background(wake) => {
                let (tx, wake) = (self.tx.clone(), wake.clone());
                std::thread::spawn(move || {
                    let _ = tx.send((id.clone(), read_image(&id)));
                    wake();
                });
            }
        }
    }

    /// What's held is read now, as it would have been.
    #[cfg(test)]
    pub fn release(&self) {
        if let How::Held(held) = &self.how {
            for id in held.take() {
                let _ = self.tx.send((id.clone(), read_image(&id)));
            }
        }
    }

    /// The next image that's been read, if any.
    pub fn next(&self) -> Option<Loaded> {
        self.rx.try_recv().ok()
    }
}
