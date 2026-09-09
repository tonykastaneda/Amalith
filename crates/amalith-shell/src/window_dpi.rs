//! Per-window physical/logical coordinate conversion. UI scale is deliberately
//! absent: chrome metrics already express their size in logical window pixels.
use vello::kurbo::{Affine, Point};

#[derive(Clone, Copy, Debug)]
pub struct WindowDpi {
    factor: f64,
}

impl WindowDpi {
    pub fn new(factor: f64) -> Self {
        assert!(factor.is_finite() && factor > 0.0);
        Self { factor }
    }

    pub fn factor(self) -> f64 {
        self.factor
    }

    pub fn point(self, x: f64, y: f64) -> Point {
        Point::new(x / self.factor, y / self.factor)
    }

    pub fn transform(self) -> Affine {
        Affine::scale(self.factor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_one_window_between_monitors_preserves_the_other() {
        let main = WindowDpi::new(2.0);
        let mut floating = WindowDpi::new(1.0);
        let logical = Point::new(120.0, 80.0);
        assert_eq!(main.point(240.0, 160.0), logical);
        assert_eq!(floating.point(120.0, 80.0), logical);
        floating = WindowDpi::new(1.5);
        assert_eq!(main.factor(), 2.0);
        for dpi in [main, floating] {
            let physical = dpi.transform() * logical;
            assert_eq!(dpi.point(physical.x, physical.y), logical);
        }
    }
}
