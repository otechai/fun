//! One error type, four ways to exit.

use std::{fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// The command line was wrong (exit 2).
    Usage(String),
    /// Something failed; the message says what and, where possible, how to fix it (exit 1).
    Fail(String),
    /// Already reported piecemeal; just set the exit code (exit 1).
    Reported,
    /// The reader of our stdout went away (`fun show x | head`). Leave quietly (exit 141).
    BrokenPipe,
}

impl Error {
    pub fn fail(msg: impl Into<String>) -> Self {
        Error::Fail(msg.into())
    }

    /// Print the error the way a CLI should: usage as-is, failures prefixed, silent ones not at all.
    pub fn report(&self) {
        match self {
            Error::Usage(m) => eprintln!("{}", m.trim_end()),
            Error::Fail(m) => eprintln!("fun: {m}"),
            Error::Reported | Error::BrokenPipe => {}
        }
    }

    pub fn code(&self) -> u8 {
        match self {
            Error::Usage(_) => 2,
            Error::Fail(_) | Error::Reported => 1,
            Error::BrokenPipe => 141,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(m) | Error::Fail(m) => f.write_str(m),
            Error::Reported | Error::BrokenPipe => Ok(()),
        }
    }
}

/// `io::Result<T>` -> `Result<T>`, saying what we were doing when it went wrong.
pub trait Context<T> {
    fn context(self, what: impl FnOnce() -> String) -> Result<T>;
}

impl<T> Context<T> for io::Result<T> {
    fn context(self, what: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|e| match e.kind() {
            io::ErrorKind::BrokenPipe => Error::BrokenPipe,
            _ => Error::Fail(format!("{}: {e}", what())),
        })
    }
}
