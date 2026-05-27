//! Serialize tests that mutate `CTX_HOME` so parallel runs do not clobber each other.

use std::sync::Mutex;

pub static CTX_ENV_LOCK: Mutex<()> = Mutex::new(());
