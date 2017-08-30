//! A zero-config simple histogram collector. ~160ns/collection with a random input,
//! ~65ns/collection on already existing metrics. Uses logarithmic bucketing
//! rather than sampling have bounded (generally <0.5%) error percentiles.
#![deny(missing_docs)]
#![cfg_attr(test, deny(warnings))]

extern crate coco;

use radix::Radix;

pub use histo::Histo;

macro_rules! rep_no_copy {
    ($e:expr; $n:expr) => {
        {
            let mut v = Vec::with_capacity($n);
            for _ in 0..$n {
                v.push($e);
            }
            v
        }
    };
}

mod radix;
mod histo;
