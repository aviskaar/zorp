//! Job definitions, schedule arithmetic and run bookkeeping for zorp's
//! scheduled jobs.
//!
//! This crate is deliberately inert. It parses job files, works out when a
//! job is next due, decides what to do about occurrences that were missed
//! while the machine was off, takes a per-job lock, and appends run outcomes
//! to a history file. It never starts an agent, never runs a command, and
//! never decides that something is approved. Those live in `zorp-agent`,
//! which is where the approval model already lives and is the only sensible
//! place for it to stay.
//!
//! Every piece of time reasoning here takes its clock and its time zone as
//! arguments. Nothing calls the system clock except `SystemZone`, so the
//! whole of schedule computation, missed-run policy and daylight saving
//! behaviour is testable by pinning time rather than by waiting for it.

pub mod civil;
pub mod clock;
pub mod due;
pub mod history;
pub mod job;
pub mod lock;
pub mod schedule;

pub use civil::{Civil, Weekday};
pub use clock::{Clock, FixedClock, SystemClock, SystemZone, TimeZone};
pub use due::{initial_watermark, plan, DuePlan};
pub use history::{state_root, History, RunOutcome, RunRecord};
pub use job::{Job, JobFile, JobPreset, OnMissed};
pub use lock::{JobLock, LockError};
pub use schedule::{Schedule, ScheduleError};

/// Shared boxed error, mirroring the rest of the workspace so `?` composes.
pub type BoxErr = Box<dyn std::error::Error + Send + Sync>;
