//! Monitor actions, split by concern:
//! - `set` — old_value_for, set_action, try_action
//! - `lid` — lid_closed, clear_disabled, internal/mirror/lid
//! - `recover` — recover/keep/revert
//!
//! Re-exported here so `actions::*` callers are unchanged.

mod set;
mod lid;
mod recover;

pub(crate) use set::{set_action, try_action};
pub(crate) use lid::{internal_action, lid_action, mirror_action};
pub(crate) use recover::{keep_action, recover_action, revert_action};
