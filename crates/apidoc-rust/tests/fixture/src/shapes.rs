//! Shapes.

use crate::{Area, Point};

/// A circle, see [`Area`] and [`crate::total`].
#[derive(Debug)]
pub struct Circle {
    /// Where.
    pub center: Point,
    /// How big.
    pub radius: f64,
}

impl Area for Circle {
    type Unit = f64;

    fn area(&self) -> f64 {
        std::f64::consts::PI * self.radius * self.radius
    }
}

/// A square of a side.
#[derive(Debug)]
pub struct Square(pub f64);

impl Area for Square {
    type Unit = f64;
    const SIDES: usize = 4;

    fn area(&self) -> f64 {
        self.0 * self.0
    }
}
