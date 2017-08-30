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

impl Debug for Histo {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        const PS: [f64; 10] = [0., 50., 75., 90., 95., 97.5, 99., 99.9, 99.99, 100.];
        f.write_str("Histogram[")?;

        for p in &PS {
            let res = self.percentile::<u64>(*p);
            let line = format!("({} -> {:?}) ", p, res);
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

    /// Retrieve a percentile. Returns None if no metrics have been
    /// collected yet.
    pub fn percentile<T: From<u64>>(&self, p: f64) -> Option<T> {
        assert!(p <= 100.);

        let set = self.vals.read().unwrap();

        let target = self.total.load(Ordering::Acquire) as f64 * (p / 100.);
        let mut total = 0.;

        for val in &*set {
            let ptr = self.inner.get(*val);
            let count = unsafe { (*ptr).load(Ordering::Acquire) };
            total += count as f64;

            if total >= target {
                return Some(decompress(*val));
            }
        }

        None
    }

    /// Dump out some common percentiles.
    pub fn print_percentiles(&self) {
        println!("{:?}", self);
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
// the original float64 passed to compress.
fn decompress<T: From<u64>>(compressed: u16) -> T {
    let compressed: f64 = compressed.into();
    let abs = compressed.abs();
    let unboosted = abs / PRECISION;
    let exp = unboosted.exp();
    let decompressed = (exp - 1.) as u64;
    decompressed.into()
}

#[test]
fn it_works() {
    let c = Histo::default();
    assert_eq!(c.measure(2), 1);
    assert_eq!(c.measure(2), 2);
    assert_eq!(c.measure(3), 1);
    assert_eq!(c.measure(3), 2);
    assert_eq!(c.measure(4), 1);
    assert_eq!(c.percentile(0.), Some(2_u64));
    assert_eq!(c.percentile(40.), Some(2_u64));
    assert_eq!(c.percentile(40.1), Some(3_u64));
    assert_eq!(c.percentile(80.), Some(3_u64));
    assert_eq!(c.percentile(80.1), Some(4_u64));
    assert_eq!(c.percentile(100.), Some(4_u64));
    c.print_percentiles();
}
