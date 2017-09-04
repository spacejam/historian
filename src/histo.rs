use std::collections::BTreeSet;
use std::fmt::{self, Debug};
use std::sync::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

const PRECISION: f64 = 100.;

/// A histogram collector that uses zero-configuration logarithmic buckets.
#[derive(Default)]
pub struct Histo {
    inner: Radix,
    vals: RwLock<BTreeSet<u16>>,
    total: AtomicUsize,
}

unsafe impl Send for Histo {}

impl Debug for Histo {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        const PS: [f64; 10] = [0., 50., 75., 90., 95., 97.5, 99., 99.9, 99.99, 100.];
        f.write_str("Histogram[")?;

        for p in &PS {
            let res = self.percentile(*p).round();
            let line = format!("({} -> {}) ", p, res);
            f.write_str(&*line)?;
        }

        f.write_str("]")
    }
}

impl Histo {
    /// Record a value.
    pub fn measure<T: Into<f64>>(&self, value: T) -> usize {
        self.total.fetch_add(1, Ordering::Relaxed);

        let compressed = compress(value);
        self.ensure(compressed);

        let counter = self.inner.get(compressed);
        let old = unsafe { (*counter).fetch_add(1, Ordering::Release) };
        old + 1
    }

    /// Retrieve a percentile [0-100]. Returns NAN if no metrics have been
    /// collected yet.
    pub fn percentile(&self, p: f64) -> f64 {
        assert!(p <= 100.);

        let set = self.vals.read().unwrap();

        let target = self.total.load(Ordering::Acquire) as f64 * (p / 100.);
        let mut total = 0.;

        for val in &*set {
            let ptr = self.inner.get(*val);
            let count = unsafe { (*ptr).load(Ordering::Acquire) };
            total += count as f64;

            if total >= target {
                return decompress(*val);
            }
        }

        std::f64::NAN
    }

    /// Dump out some common percentiles.
    pub fn print_percentiles(&self) {
        println!("{:?}", self);
    }

    /// Return the total number of observations in this histogram.
    pub fn count(&self) -> usize {
        self.total.load(Ordering::Acquire)
    }

    fn ensure(&self, value: u16) {
        {
            let set = self.vals.read().unwrap();
            if set.contains(&value) {
                return;
            }
        }

        let mut set = self.vals.write().unwrap();
        set.insert(value);
    }
}

// compress takes a value and lossily shrinks it to an u16 to facilitate
// bucketing of histogram values, staying roughly within 1% of the true
// value. This fails for large values of 1e142 and above, and is
// inaccurate for values closer to 0 than +/- 0.51 or +/- math.Inf.
fn compress<T: Into<f64>>(value: T) -> u16 {
    let value: f64 = value.into();
    let abs = value.abs();
    let boosted = 1. + abs;
    let ln = boosted.ln();
    let compressed = PRECISION * ln + 0.5;
    assert!(compressed <= std::u16::MAX as f64);
    compressed as u16
}

// decompress takes a lossily shrunken u16 and returns an f64 within 1% of
// the original passed to compress.
fn decompress(compressed: u16) -> f64 {
    let unboosted = compressed as f64 / PRECISION;
    (unboosted.exp() - 1.)
}

#[test]
fn it_works() {
    let c = Histo::default();
    assert_eq!(c.measure(2), 1);
    assert_eq!(c.measure(2), 2);
    assert_eq!(c.measure(3), 1);
    assert_eq!(c.measure(3), 2);
    assert_eq!(c.measure(4), 1);
    assert_eq!(c.percentile(0.).round() as usize, 2);
    assert_eq!(c.percentile(40.).round() as usize, 2);
    assert_eq!(c.percentile(40.1).round() as usize, 3);
    assert_eq!(c.percentile(80.).round() as usize, 3);
    assert_eq!(c.percentile(80.1).round() as usize, 4);
    assert_eq!(c.percentile(100.).round() as usize, 4);
    c.print_percentiles();
}

#[test]
fn high_percentiles() {
    let c = Histo::default();
    for _ in 0..9000 {
        c.measure(10);
    }
    for _ in 0..900 {
        c.measure(25);
    }
    for _ in 0..90 {
        c.measure(33);
    }
    for _ in 0..9 {
        c.measure(47);
    }
    c.measure(500);
    assert_eq!(c.percentile(0.).round() as usize, 10);
    assert_eq!(c.percentile(99.).round() as usize, 25);
    assert_eq!(c.percentile(99.89).round() as usize, 33);
    assert_eq!(c.percentile(99.91).round() as usize, 47);
    assert_eq!(c.percentile(99.99).round() as usize, 47);
    assert_eq!(c.percentile(100.).round() as usize, 502);
}

#[test]
fn multithreaded() {
    use std::thread;
    use std::sync::Arc;

    let h = Arc::new(Histo::default());
    let mut threads = vec![];

    for _ in 0..10 {
        let h = h.clone();
        threads.push(thread::spawn(move || { h.measure(20); }));
    }

    for t in threads.into_iter() {
        t.join().unwrap();
    }

    assert_eq!(h.percentile(50.).round() as usize, 20);
}
