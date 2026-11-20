//! What every machine is (#110): a [`Machine`] decides what an event does to its state and says
//! what its actor carries out next, and the harness tests one the same way each time.
//!
//! Each machine's own module names its parts alike: `State`, `Event` (`Command(..)` or
//! `Observed(..)`), `Do`, `decide` (one match over every state and event, no `_` arm) and `next`.
//! [`Machine`] is those as a trait, so the harness can take any of them.
//!
//! The harness (feature `harness`): a `table` over every state × every input, each pair in
//! exactly one `Row`, whose property its outcome must have, and properties of every pair; and
//! `sequences` of generated events from given states, checking the same rows on every state
//! they reach. "Every" comes from [`Every`], made with [`every!`](crate::every): a new variant
//! doesn't compile until it has samples, so a new state or event can't skip the table.

/// A state machine: pure, the actor that owns its state carries out what it says.
pub trait Machine {
    type State;
    type Event;
    /// Why a command is refused: data, which the client words.
    type Refusal;
    /// What `next` looks at besides the state: what the outside is doing now.
    type World;
    /// What the actor carries out.
    type Do;

    /// What `event` does to `state`. Exhaustive: every pair written out.
    fn decide(state: &Self::State, event: &Self::Event) -> Result<Self::State, Self::Refusal>;
    /// What makes the world match `state`; level-triggered, so asking again is safe.
    fn next(state: &Self::State, world: &Self::World) -> Option<Self::Do>;
}

/// What [`Machine::decide`] says.
pub type Decided<M> = Result<<M as Machine>::State, <M as Machine>::Refusal>;

/// Samples of every variant of a type, for tables. Made with [`every!`](crate::every), or by
/// hand for a struct, destructuring it so a new field doesn't compile.
pub trait Every: Sized {
    fn every() -> Vec<Self>;
}

/// `every!(T { pattern => samples, .. })`: [`Every`] for `T`, with these samples for each pattern.
/// The patterns must cover `T` (a new variant fails to compile here), every sample must match its
/// pattern, and every pattern have one at least; `samples` is anything iterable.
#[macro_export]
macro_rules! every {
    ($t:ty { $($pattern:pat => $samples:expr),+ $(,)? }) => {
        impl $crate::Every for $t {
            fn every() -> Vec<Self> {
                #[allow(dead_code)]
                fn covers(sample: &$t) {
                    match sample {
                        $($pattern => {})+
                    }
                }
                let mut all = Vec::new();
                $(
                    let some: Vec<$t> = ::std::iter::IntoIterator::into_iter($samples).collect();
                    assert!(!some.is_empty(), "no sample of {}", stringify!($pattern));
                    for sample in &some {
                        assert!(matches!(sample, $pattern), "{sample:?} isn't {}", stringify!($pattern));
                    }
                    all.extend(some);
                )+
                all
            }
        }
    };
}

#[cfg(feature = "harness")]
pub use harness::*;

#[cfg(feature = "harness")]
mod harness {
    use std::fmt::Debug;

    use proptest::test_runner::{Config, TestRunner};

    use super::{Decided, Every, Machine};

    /// One row of a table: of the pairs `when` selects, what `f` says has property `then`.
    pub struct Row<S, I, O> {
        pub says: &'static str,
        pub when: fn(&S, &I) -> bool,
        pub then: fn(&S, &I, &O) -> bool,
    }

    /// A row of a machine's `decide` table.
    pub type DecideRow<M> = Row<<M as Machine>::State, <M as Machine>::Event, Decided<M>>;

    impl<S, I, O> Row<S, I, O> {
        /// A property of every pair.
        pub fn always(says: &'static str, then: fn(&S, &I, &O) -> bool) -> Self {
            Self { says, when: |_, _| true, then }
        }
    }

    /// `state` and `input` are in exactly one of `rows`, and `f`'s outcome has its property and
    /// every one of `always`.
    pub fn check<S: Debug, I: Debug, O: Debug>(
        f: impl Fn(&S, &I) -> O,
        rows: &[Row<S, I, O>],
        always: &[Row<S, I, O>],
        state: &S,
        input: &I,
    ) -> O {
        let mut matched = rows.iter().filter(|row| (row.when)(state, input));
        let row = matched.next().unwrap_or_else(|| panic!("no row for {input:?} on {state:?}"));
        if let Some(other) = matched.next() {
            panic!("{input:?} on {state:?} is in two rows: {:?} and {:?}", row.says, other.says);
        }
        let out = f(state, input);
        for row in std::iter::once(row).chain(always) {
            assert!((row.then)(state, input, &out), "{}: {input:?} on {state:?} is {out:?}", row.says);
        }
        out
    }

    /// Every state × every input, as [`check`] says.
    pub fn table<S: Every + Debug, I: Every + Debug, O: Debug>(f: impl Fn(&S, &I) -> O, rows: &[Row<S, I, O>], always: &[Row<S, I, O>]) {
        let inputs = I::every();
        for state in S::every() {
            for input in &inputs {
                check(&f, rows, always, &state, input);
            }
        }
    }

    /// [`table`] of `M`'s `decide`.
    pub fn decide_table<M: Machine>(rows: &[DecideRow<M>], always: &[DecideRow<M>])
    where
        M::State: Every + Debug,
        M::Event: Every + Debug,
        M::Refusal: Debug,
    {
        table(M::decide, rows, always);
    }

    /// Generated sequences of `M`'s events, up to `len` long, from each of `starts`: every state
    /// they reach, with every event after it, is [`check`]ed. What a refused command leaves is the
    /// state it was refused on. `PROPTEST_CASES` cases (default 256), as everywhere.
    ///
    /// A failing sequence is saved to the calling module's file in `proptest-regressions/`, as a
    /// `proptest!` test's is, and replayed first on the next run.
    #[track_caller]
    pub fn sequences<M: Machine>(starts: Vec<M::State>, len: usize, rows: &[DecideRow<M>], always: &[DecideRow<M>])
    where
        M::State: Clone + Debug + 'static,
        M::Event: Every + Clone + Debug + 'static,
        M::Refusal: Debug,
    {
        // The same text `file!()` gives in the caller, which `proptest!` passes on.
        let source_file = std::panic::Location::caller().file();
        sequences_in::<M>(Config::default(), source_file, starts, len, rows, always);
    }

    pub(crate) fn sequences_in<M: Machine>(
        config: Config,
        source_file: &'static str,
        starts: Vec<M::State>,
        len: usize,
        rows: &[DecideRow<M>],
        always: &[DecideRow<M>],
    ) where
        M::State: Clone + Debug + 'static,
        M::Event: Every + Clone + Debug + 'static,
        M::Refusal: Debug,
    {
        let events = M::Event::every();
        let strategy = (proptest::sample::select(starts), proptest::collection::vec(proptest::sample::select(events), 0..=len));
        let mut runner = TestRunner::new(Config { source_file: Some(source_file), ..config });
        runner
            .run(&strategy, |(start, events)| {
                let mut state = start;
                for event in &events {
                    if let Ok(next) = check(M::decide, rows, always, &state, event) {
                        state = next;
                    }
                }
                Ok(())
            })
            .unwrap();
    }
}

#[cfg(all(test, feature = "harness"))]
mod tests {
    use std::path::Path;

    use proptest::test_runner::Config;

    use super::{harness::sequences_in, *};

    /// A counter that any event pushes up: every sequence but the empty one fails its row.
    struct Counter;

    #[derive(Clone, Debug, PartialEq)]
    enum Event {
        Up,
    }

    crate::every!(Event { Event::Up => [Event::Up] });

    impl Machine for Counter {
        type State = u8;
        type Event = Event;
        type Refusal = ();
        type World = ();
        type Do = ();

        fn decide(state: &u8, _: &Event) -> Result<u8, ()> {
            Ok(state.saturating_add(1))
        }

        fn next(_: &u8, _: &()) -> Option<()> {
            None
        }
    }

    fn never_up() -> Vec<DecideRow<Counter>> {
        vec![Row::always("an event leaves the state", |state, _, out| out.as_ref() == Ok(state))]
    }

    /// A crate with a `src/lib.rs` and `src/<module>.rs`, whose regressions file is
    /// `proptest-regressions/<module>.txt`.
    fn crate_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        for file in ["lib.rs", "module.rs"] {
            std::fs::write(dir.path().join("src").join(file), "").unwrap();
        }
        dir
    }

    fn run(config: Config, module: &Path) -> bool {
        let source: &'static str = Box::leak(module.to_str().unwrap().to_owned().into_boxed_str());
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sequences_in::<Counter>(config, source, vec![0], 3, &never_up(), &[])))
            .is_ok()
    }

    #[test]
    fn a_failing_sequence_is_saved_next_to_its_caller_and_replayed() {
        let dir = crate_dir();
        let module = dir.path().join("src/module.rs");
        let saved = dir.path().join("proptest-regressions/module.txt");
        // With no case to generate, only a saved seed can make the run fail.
        let replay_only = || Config { cases: 0, ..Config::default() };

        assert!(run(replay_only(), &module), "nothing saved yet, nothing to replay");
        assert!(!run(Config::default(), &module), "a sequence fails");
        let file = std::fs::read_to_string(&saved).expect("the seed is saved beside the caller");
        assert!(file.lines().any(|line| line.starts_with("cc ")), "{file}");
        assert!(!run(replay_only(), &module), "the next run replays the saved seed");
    }
}
