//! Win32 adapters for the dictation core. Pure helpers build everywhere so they can be tested on
//! any machine; the Win32 parts only build on Windows.

pub mod hook;
pub mod hotkey;
pub mod inserter;
pub mod platform;
pub mod uia;
