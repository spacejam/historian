# historian

[![Build Status](https://travis-ci.org/spacejam/historian.svg?branch=master)](https://travis-ci.org/spacejam/historian)
[![crates.io](http://meritbadge.herokuapp.com/historian)](https://crates.io/crates/historian)
[![documentation](https://docs.rs/historian/badge.svg)](https://docs.rs/historian)

A zero-config simple histogram collector. ~160ns/collection with a random input,
~65ns/collection on already existing metrics. Uses logarithmic bucketing
rather than sampling have bounded (generally <0.5%) error percentiles.
