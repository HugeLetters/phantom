//! Property tests whose cases are slow because they wait (each runs an engine): their cases run
//! side by side, on several runners. Not for cases that compute: those gain no time, and cost CPU. [`proptest!`] and [`prop_state_machine!`] take
//! the form of proptest's macros of those names; [`side_by_side`] is what they run.

use proptest::strategy::Strategy;
use proptest::test_runner::{Config, FailurePersistence, PersistedSeed, TestCaseResult, TestRunner};

#[doc(hidden)]
pub use {proptest as __proptest, proptest_state_machine as __state_machine};

/// How many runners at most.
const RUNNERS: u32 = 8;

/// Runs `test` on `config.cases` cases from `strategy`, split over up to [`RUNNERS`] runners side by
/// side, at least 2 cases each: each its own thread, generator and share of the cases, run as
/// `proptest!` runs them. The seeds saved for `config.source_file` are replayed once, by the first
/// runner; a failure of any is saved. The first failure, shrunk, fails the test once every runner
/// is done.
pub fn side_by_side<S: Strategy>(config: Config, strategy: impl Fn() -> S + Sync, test: impl Fn(S::Value) -> TestCaseResult + Sync) {
    let runners = (config.cases / 2).clamp(1, RUNNERS);
    let failures: Vec<String> = std::thread::scope(|scope| {
        let running: Vec<_> = (0..runners)
            .map(|n| {
                let cases = config.cases / runners + u32::from(n < config.cases % runners);
                let mut config = Config { cases, ..config.clone() };
                if n > 0 {
                    config.failure_persistence = config.failure_persistence.map(|saves| Box::new(SaveOnly(saves)) as _);
                }
                let (strategy, test) = (&strategy, &test);
                scope.spawn(move || {
                    let mut runner = TestRunner::new(config);
                    runner.run(&strategy(), test).err().map(|e| format!("{e}\n{runner}"))
                })
            })
            .collect();
        running.into_iter().filter_map(|r| r.join().unwrap()).collect()
    });
    if let Some(failure) = failures.first() {
        panic!("{failure}");
    }
}

/// Saves failures as `0` does, but replays none.
#[derive(Debug, Clone)]
struct SaveOnly(Box<dyn FailurePersistence>);

impl FailurePersistence for SaveOnly {
    fn load_persisted_failures2(&self, _: Option<&'static str>) -> Vec<PersistedSeed> {
        Vec::new()
    }

    fn save_persisted_failure2(&mut self, source_file: Option<&'static str>, seed: PersistedSeed, shrunken_value: &dyn std::fmt::Debug) {
        self.0.save_persisted_failure2(source_file, seed, shrunken_value);
    }

    fn box_clone(&self) -> Box<dyn FailurePersistence> {
        Box::new(self.clone())
    }

    fn eq(&self, other: &dyn FailurePersistence) -> bool {
        other.as_any().downcast_ref::<Self>().is_some_and(|other| *self.0 == *other.0)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// proptest's `proptest!`, its cases run [`side_by_side`]. The same form: a config, then `#[test]`
/// functions of `pattern in strategy`.
#[macro_export]
macro_rules! proptest {
    ($(#[$meta:meta])* fn $($rest:tt)*) => {
        $crate::proptest! { #![proptest_config($crate::__proptest::test_runner::Config::default())] $(#[$meta])* fn $($rest)* }
    };
    (#![proptest_config($config:expr)] $($(#[$meta:meta])* fn $name:ident($($arg:pat in $strategy:expr),+ $(,)?) $body:block)*) => { $(
        $(#[$meta])*
        fn $name() {
            let config = $crate::__proptest::test_runner::Config { source_file: Some(file!()), ..$config };
            $crate::side_by_side(config, || ($($strategy,)+), |($($arg,)+)| {
                let _: () = $body;
                Ok(())
            });
        }
    )* };
}

/// proptest-state-machine's `prop_state_machine!`, its cases run [`side_by_side`]. The same form.
#[macro_export]
macro_rules! prop_state_machine {
    (#![proptest_config($config:expr)] $($(#[$meta:meta])* fn $name:ident(sequential $size:expr => $test:ty);)*) => { $(
        $(#[$meta])*
        fn $name() {
            use $crate::__state_machine::{ReferenceStateMachine as _, StateMachineTest as _};
            type Reference = <$test as $crate::__state_machine::StateMachineTest>::Reference;
            let config = $crate::__proptest::test_runner::Config { source_file: Some(file!()), ..$config };
            $crate::side_by_side(config.clone(), || Reference::sequential_strategy($size), |(initial, transitions, seen)| {
                <$test>::test_sequential(config.clone(), initial, transitions, seen);
                Ok(())
            });
        }
    )* };
}
