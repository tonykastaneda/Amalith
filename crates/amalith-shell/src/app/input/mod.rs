//! Input routing for [`App`](super::App), split by kind. `window_event`
//! (in `app/mod.rs`) is the winit entry point and delegates the big arms
//! here: presses to [`press`], pointer motion / release to [`pointer`],
//! keys to [`keyboard`], the wheel to [`scroll`].
//!
//! Every handler is an `impl App` method on `super::App`. These modules
//! are descendants of `app`, so they reach `App`'s private state and
//! helper methods without any visibility changes.

mod keyboard;
mod pointer;
mod press;
mod scroll;
