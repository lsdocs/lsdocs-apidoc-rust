//! Geometry, for testing.
//!
//! See [`Point`], [the shapes](shapes) and [`Vec`].

use std::fmt;

pub mod shapes;

mod hidden {
    /// Kept out of sight, re-exported.
    pub struct Secret(pub u8);

    /// Globbed.
    pub fn globbed() {}
}

pub use hidden::Secret as Revealed;
pub use hidden::*;
pub use shapes::Circle;
pub use std::collections::HashMap;

/// A point.
///
/// # Examples
///
/// ```
/// # use geo::Point;
/// let p = Point::new(1.0, 2.0);
/// assert_eq!(p.x, 1.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Across.
    pub x: f64,
    /// Up.
    pub y: f64,
    tag: u8,
}

impl Point {
    /// The origin.
    pub const ORIGIN: Point = Point::new(0.0, 0.0);

    /// Makes one.
    ///
    /// # Arguments
    ///
    /// * `x` - Across.
    /// * `y` - Up.
    pub const fn new(x: f64, y: f64) -> Self {
        Point { x, y, tag: 0 }
    }

    /// How far from [`Point::ORIGIN`].
    pub fn norm(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Moves it.
    pub fn shift(&mut self, by: impl Into<Point>) {
        let by = by.into();
        self.x += by.x;
        self.y += by.y;
    }

    fn private(&self) -> u8 {
        self.tag
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// Something with an area.
pub trait Area: fmt::Debug {
    /// The unit.
    type Unit;
    /// How many sides.
    const SIDES: usize = 0;

    /// The area.
    fn area(&self) -> f64;

    /// Twice the area.
    fn double(&self) -> f64 {
        2.0 * self.area()
    }
}

/// Which way.
#[repr(u8)]
pub enum Direction {
    /// Up.
    North = 1,
    /// Sideways, by how much.
    East(u32),
    /// Down, somewhere.
    South {
        /// How far.
        far: u32,
    },
}

/// Bits of a float.
pub union Bits {
    /// The float.
    pub f: f32,
    /// Its bits.
    pub u: u32,
}

/// Pairs of things.
pub type Pair<T> = (T, T);

/// The answer.
pub const ANSWER: u32 = 42;

/// A counter.
pub static mut COUNT: usize = 0;

/// Sums them.
///
/// # Errors
///
/// When empty.
///
/// # Panics
///
/// Never.
pub fn total<'a, I, A>(items: I, scale: Option<&'a [f64]>) -> Result<f64, String>
where
    I: IntoIterator<Item = &'a A>,
    A: Area + 'a,
{
    let _ = scale;
    Ok(items.into_iter().map(|a| a.area()).sum())
}

/// Calls out.
///
/// # Safety
///
/// Unchecked.
pub unsafe extern "C" fn raw(ptr: *const u8, len: usize) -> i32 {
    let _ = (ptr, len);
    0
}

/// Waits.
pub async fn later() {}

/// Old.
#[deprecated(since = "0.2.0", note = "use `total`")]
pub fn old() {}

/// Makes a point.
#[macro_export]
macro_rules! point {
    ($x:expr, $y:expr) => {
        $crate::Point::new($x, $y)
    };
}
