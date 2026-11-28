//! An enum that lists its own values, so one can't be left out of the list.

/// Declares an enum and its `ALL`, an array of one value per entry, in the order written.
///
/// A variant without data is its own entry. A variant with data says which values stand for it
/// (`Priority(Priority) => [Priority::Normal, Priority::Off]`, the payloads): leaving the `=> [...]` out doesn't
/// compile, so a new variant can't be forgotten. Every variant ends with a comma. Attributes and
/// doc comments on the enum and on its variants are kept.
macro_rules! listed_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($body:tt)* }) => {
        listed_enum!(@go [$(#[$meta])* $vis] $name; enum [] items []; $($body)*);
    };
    // A variant without data.
    (@go $head:tt $name:ident; enum [$($e:tt)*] items [$($i:tt)*];
        $(#[$vm:meta])* $v:ident, $($rest:tt)*) => {
        listed_enum!(@go $head $name; enum [$($e)* $(#[$vm])* $v,] items [$($i)* $name::$v,]; $($rest)*);
    };
    // A variant with data, and the values that stand for it.
    (@go $head:tt $name:ident; enum [$($e:tt)*] items [$($i:tt)*];
        $(#[$vm:meta])* $v:ident($ty:ty) => [$($item:expr),+ $(,)?], $($rest:tt)*) => {
        listed_enum!(@go $head $name; enum [$($e)* $(#[$vm])* $v($ty),] items [$($i)* $($name::$v($item),)+]; $($rest)*);
    };
    (@go [$(#[$meta:meta])* $vis:vis] $name:ident; enum [$($e:tt)*] items [$($item:expr,)*];) => {
        $(#[$meta])*
        $vis enum $name { $($e)* }

        impl $name {
            /// One of each, in the order the enum lists them.
            pub const ALL: [$name; <[()]>::len(&[$(listed_enum!(@unit $item)),*])] = [$($item),*];
        }
    };
    (@unit $item:expr) => { () };
}
pub(crate) use listed_enum;
