//! Keeping the Mac awake while a torrent downloads (#122). An actor owns macOS's power assertion
//! (`PreventUserIdleSystemSleep`: the Mac doesn't go to sleep on its own, the display still may);
//! its [`machine`] decides from what the service tells it: the setting, whether some torrent is
//! downloading (from each of the engine's snapshots), and the service stopping. The service owns
//! it, not a client: it runs without the GUI.

pub mod machine;

use std::sync::Mutex;

use anyhow::{Result, bail};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use machine::{Assertion, Command, Do, Event, Observed, State};

/// The handle: every call is a message to the actor's task.
pub struct KeepAwake {
    events: mpsc::UnboundedSender<Event>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl KeepAwake {
    /// Starts the actor with the setting `on`, nothing downloading yet, holding through `power`.
    pub fn start(on: bool, power: impl Power + Send + 'static) -> Self {
        let (events, mut inbox) = mpsc::unbounded_channel();
        let mut core = Core::new(on, power);
        let task = tokio::spawn(async move {
            while let Some(event) = inbox.recv().await {
                core.handle(event);
                if core.state == State::Stopped {
                    return;
                }
            }
        });
        Self { events, task: Mutex::new(Some(task)) }
    }

    pub fn set(&self, on: bool) {
        let _ = self.events.send(Event::Command(Command::Set(on)));
    }

    pub fn downloading(&self, downloading: bool) {
        let _ = self.events.send(Event::Observed(Observed::Downloading(downloading)));
    }

    /// Releases the assertion for good; returns once it's released.
    pub async fn stop(&self) {
        let _ = self.events.send(Event::Command(Command::Stop));
        let task = self.task.lock().unwrap().take();
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}

/// The actor's state and what it holds, without the task: handles one event at a time.
struct Core<P> {
    state: State,
    assertion: Assertion,
    power: P,
    /// macOS refused the last hold: the next refusals in a row aren't logged again.
    refused: bool,
}

impl<P: Power> Core<P> {
    fn new(on: bool, power: P) -> Self {
        Self { state: State::Running { on, downloading: false }, assertion: Assertion::Released, power, refused: false }
    }

    /// Decides `event`, then makes the assertion what the state wants. A hold that fails is
    /// asked again at the next event, and logged once per run of refusals.
    fn handle(&mut self, event: Event) {
        let Ok(state) = machine::decide(&self.state, &event);
        self.state = state;
        if let Some(todo) = machine::next(&self.state, &self.assertion) {
            match todo {
                Do::Hold => match self.power.hold() {
                    Ok(()) => {
                        info!("keeping the Mac awake: a torrent is downloading");
                        self.assertion = Assertion::Held;
                        self.refused = false;
                    }
                    Err(e) => {
                        if !self.refused {
                            warn!(error = %format!("{e:#}"), "can't keep the Mac awake; asking again at each change");
                        }
                        self.refused = true;
                    }
                },
                Do::Release => {
                    self.power.release();
                    info!("letting the Mac sleep");
                    self.assertion = Assertion::Released;
                }
            }
        }
    }
}

/// Where the assertion is held: macOS ([`MacOs`]), or a pretend one in tests.
pub trait Power {
    fn hold(&mut self) -> Result<()>;
    fn release(&mut self);
}

/// macOS's power assertion, through IOKit; released when dropped, and by macOS when the process
/// ends.
#[derive(Default)]
pub struct MacOs {
    /// The assertion's id, while held.
    held: Option<u32>,
}

/// What `pmset -g assertions` and Activity Monitor show as the reason.
const NAME: &str = "Phantom is downloading";

impl Power for MacOs {
    fn hold(&mut self) -> Result<()> {
        if self.held.is_some() {
            return Ok(());
        }
        let id = iokit::prevent_idle_sleep(NAME)?;
        self.held = Some(id);
        Ok(())
    }

    fn release(&mut self) {
        if let Some(id) = self.held.take()
            && let Err(e) = iokit::release(id)
        {
            warn!(error = %e, "can't release the power assertion");
        }
    }
}

impl Drop for MacOs {
    fn drop(&mut self) {
        self.release();
    }
}

/// The two IOKit calls, declared here: `IOPMAssertionCreateWithName` and `IOPMAssertionRelease`
/// (`IOKit/pwr_mgt/IOPMLib.h`). The strings are CoreFoundation's, from `core-foundation`.
mod iokit {
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};

    use super::{Result, bail};

    type IOReturn = i32;
    const SUCCESS: IOReturn = 0;
    /// `kIOPMAssertionLevelOn`.
    const LEVEL_ON: u32 = 255;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(kind: CFStringRef, level: u32, name: CFStringRef, id: *mut u32) -> IOReturn;
        fn IOPMAssertionRelease(id: u32) -> IOReturn;
    }

    /// Holds `kIOPMAssertionTypePreventUserIdleSystemSleep` under `name`: its id.
    pub fn prevent_idle_sleep(name: &str) -> Result<u32> {
        let kind = CFString::from_static_string("PreventUserIdleSystemSleep");
        let name = CFString::new(name);
        let mut id = 0;
        // SAFETY: both strings live through the call, and `id` is a valid place for the id.
        let returned = unsafe { IOPMAssertionCreateWithName(kind.as_concrete_TypeRef(), LEVEL_ON, name.as_concrete_TypeRef(), &mut id) };
        if returned != SUCCESS {
            bail!("IOPMAssertionCreateWithName returned {returned:#x}");
        }
        Ok(id)
    }

    pub fn release(id: u32) -> Result<()> {
        // SAFETY: takes any id; one that isn't held is an error code.
        let returned = unsafe { IOPMAssertionRelease(id) };
        if returned != SUCCESS {
            bail!("IOPMAssertionRelease returned {returned:#x}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use proptest::prelude::*;

    use super::*;

    /// A pretend assertion: whether it's held, how many times it was taken, and whether the next
    /// hold fails.
    #[derive(Default)]
    struct Pretend {
        held: bool,
        holds: usize,
        fail_next: bool,
    }

    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Pretend>>);

    impl Power for Shared {
        fn hold(&mut self) -> Result<()> {
            let mut p = self.0.lock().unwrap();
            assert!(!p.held, "held twice");
            if std::mem::take(&mut p.fail_next) {
                bail!("refused");
            }
            p.held = true;
            p.holds += 1;
            Ok(())
        }

        fn release(&mut self) {
            let mut p = self.0.lock().unwrap();
            assert!(p.held, "released without holding");
            p.held = false;
        }
    }

    #[derive(Debug, Clone)]
    enum Step {
        Event(Event),
        /// The next hold fails, as when macOS refuses it.
        HoldFails,
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            3 => any::<bool>().prop_map(|on| Step::Event(Event::Command(Command::Set(on)))),
            6 => any::<bool>().prop_map(|d| Step::Event(Event::Observed(Observed::Downloading(d)))),
            1 => Just(Step::Event(Event::Command(Command::Stop))),
            1 => Just(Step::HoldFails),
        ]
    }

    proptest! {
        /// After every event the assertion is held exactly while the setting is on, some torrent
        /// downloads and the service hasn't stopped, but for a hold macOS refused, which the next
        /// event asks again; never held twice, never released unheld; none left once stopped.
        #[test]
        fn held_exactly_while_on_and_downloading(on in any::<bool>(), steps in proptest::collection::vec(step(), 0..40)) {
            let power = Shared::default();
            let mut core = Core::new(on, power.clone());
            // The reference: the setting, whether something downloads, stopped.
            let (mut on, mut downloading, mut stopped) = (on, false, false);
            for step in steps {
                let event = match step {
                    Step::HoldFails => {
                        power.0.lock().unwrap().fail_next = true;
                        continue;
                    }
                    Step::Event(event) => event,
                };
                match event {
                    Event::Command(Command::Set(now)) => on = now,
                    Event::Observed(Observed::Downloading(now)) => downloading = now,
                    Event::Command(Command::Stop) => stopped = true,
                }
                let refused = power.0.lock().unwrap().fail_next;
                core.handle(event);
                let p = power.0.lock().unwrap();
                let wanted = on && downloading && !stopped;
                let failed = refused && !p.fail_next;
                prop_assert_eq!(p.held, wanted && !failed, "after {:?}", event);
                prop_assert_eq!(core.assertion == Assertion::Held, p.held);
            }
        }
    }

    /// The handle, on its task: a stop returns once released.
    #[tokio::test]
    async fn a_stop_returns_once_released() {
        let power = Shared::default();
        let awake = KeepAwake::start(true, power.clone());
        awake.downloading(true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !power.0.lock().unwrap().held {
            assert!(std::time::Instant::now() < deadline, "never held");
            tokio::task::yield_now().await;
        }
        awake.stop().await;
        assert!(!power.0.lock().unwrap().held);
        awake.downloading(true);
        assert!(!power.0.lock().unwrap().held, "nothing after a stop");
    }

    /// macOS itself, which can't be generated: the assertion is listed under this process while
    /// held, and gone once released.
    #[test]
    fn macos_lists_the_assertion_while_held() {
        let listed = || {
            let out = std::process::Command::new("pmset").args(["-g", "assertions"]).output().unwrap();
            let pid = format!("pid {}(", std::process::id());
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|l| l.contains(&pid) && l.contains("PreventUserIdleSystemSleep") && l.contains(NAME))
        };
        let mut power = MacOs::default();
        power.hold().unwrap();
        assert!(listed(), "held");
        power.release();
        assert!(!listed(), "released");
        power.hold().unwrap();
        drop(power);
        assert!(!listed(), "released when dropped");
    }
}
