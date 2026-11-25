//! What drawing knows of the disk: whether a path was there when last looked at. Drawing never
//! looks itself, as a drive that stopped answering would hang the window: each look has a thread of
//! its own, and the app is woken once its answer differs from the last.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long an answer stands before a frame that needs it has the path looked at again.
const STANDS: Duration = Duration::from_secs(1);
/// A path no frame asked of for this long is forgotten.
const KEPT: Duration = Duration::from_secs(60);

struct Look {
    there: bool,
    /// When it was last asked of, and last answered; `None`: not yet.
    asked: Instant,
    answered: Option<Instant>,
    /// A thread is looking: no second one starts, however long it takes.
    looking: bool,
}

#[derive(Default)]
struct Known {
    paths: BTreeMap<PathBuf, Look>,
    /// An answer differs from what was last drawn.
    news: bool,
}

/// The latest looks, by path.
pub struct Looks {
    known: Arc<Mutex<Known>>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Looks {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self { known: Arc::default(), wake: Arc::new(wake) }
    }

    /// Whether `path` was there at the last look (`false` before the first answer). Never waits for
    /// the disk: `look` runs on a thread of its own, if the last answer is old.
    pub fn there(&self, path: &Path, look: impl FnOnce(&Path) -> bool + Send + 'static) -> bool {
        let now = Instant::now();
        let mut known = self.known.lock().unwrap();
        known.paths.retain(|p, l| p == path || l.looking || now.duration_since(l.asked) < KEPT);
        let entry = known.paths.entry(path.to_owned()).or_insert(Look { there: false, asked: now, answered: None, looking: false });
        entry.asked = now;
        let there = entry.there;
        if !entry.looking && entry.answered.is_none_or(|at| now.duration_since(at) >= STANDS) {
            entry.looking = true;
            let (known, wake, path) = (self.known.clone(), self.wake.clone(), path.to_owned());
            std::thread::spawn(move || {
                let there = look(&path);
                let mut known = known.lock().unwrap();
                let Some(entry) = known.paths.get_mut(&path) else { return };
                let news = entry.there != there;
                (entry.there, entry.answered, entry.looking) = (there, Some(Instant::now()), false);
                known.news |= news;
                drop(known);
                if news {
                    wake();
                }
            });
        }
        there
    }

    /// Whether an answer came that a frame hasn't drawn yet; asked once per frame.
    pub fn news(&self) -> bool {
        std::mem::take(&mut self.known.lock().unwrap().news)
    }
}

#[cfg(test)]
mod tests {
    use super::Looks;
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    /// A drive that doesn't answer holds up nobody: the frame gets the last answer, one thread
    /// waits, and the answer that comes wakes the app.
    #[test]
    fn a_look_never_waits_for_the_disk_and_wakes_the_app_with_its_answer() {
        let (woke, wakes) = mpsc::channel();
        let looks = Looks::new(move || woke.send(()).unwrap());
        let (answer, dead) = mpsc::channel::<()>();
        let path = Path::new("/Volumes/Dead/file");
        assert!(!looks.there(path, move |_| dead.recv().is_ok()), "not known yet");
        assert!(!looks.there(path, |_| panic!("one look at a time")));
        assert!(!looks.news());
        answer.send(()).unwrap();
        wakes.recv_timeout(Duration::from_secs(10)).expect("woken");
        assert!(looks.news() && !looks.news(), "told once");
        assert!(looks.there(path, |_| panic!("the answer still stands")));
    }
}
