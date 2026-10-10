/*
 *  state.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What the shell holds between commands, and what a refused command looks like.
 *
 * MIT License
 *
 * Copyright (c) 2026 Stue Hunter
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */

//! What the shell holds between commands, and what a refused command looks like.
//!
//! §2 puts no authoritative state in the frontend, so it has to live somewhere,
//! and this is the only place in the shell that has any. It is deliberately
//! thin: a running engine, the project it was armed on, an audition if one is
//! playing, and a bus. Everything a person can see is read back out of the
//! project or arrives as an event - the shell caches nothing, because a cache
//! here would be a second copy of the truth and §2's whole point is that there
//! is one.
//!
//! The mutexes are held for the length of a command and never across a device
//! callback. Nothing in here is locked by the audio path: the engine owns its
//! own thread and its own ring, and the shell only sends it words.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Serialize, Serializer};
use vcw_contract::command::Failure;
use vcw_core::{Bus, Engine};
use vcw_metadata::Cancel;

use crate::audition::Playing;

/// Everything the shell holds.
#[derive(Debug, Default)]
pub struct Shell {
    /// The engine, once something has armed it.
    ///
    /// `None` until then, rather than started eagerly: an engine thread with
    /// nothing to do is harmless, but the window should open on a machine with
    /// no sound card at all.
    pub engine: Mutex<Option<Engine>>,
    /// The project the engine was armed on, kept so a read command knows which
    /// file to open without the frontend passing the path back every time.
    pub project: Mutex<Option<PathBuf>>,
    /// The audition, if something is playing.
    pub playing: Mutex<Option<Playing>>,
    /// The metadata search in flight, if there is one.
    ///
    /// The one piece of shell state that is not a copy of anything, and the
    /// exception that proves the rule above: a cancellation token *is* the
    /// thing itself rather than a cache of it, because there is nowhere else a
    /// half-finished network request could be recorded. Starting a search
    /// cancels whatever was here, which is what a person retyping an album
    /// title means (§28).
    pub searching: Mutex<Option<Cancel>>,
    /// The bus playback and export publish on.
    ///
    /// Separate from the engine's, because the engine owns its own and an
    /// audition can happen when nothing is armed. Both are forwarded to the
    /// webview by [`crate::pump`], which is what makes them one stream as far
    /// as the frontend is concerned.
    pub bus: Bus,
}

impl Shell {
    /// The project path, or a refusal naming the fact that nothing is open.
    pub fn project_path(&self) -> Result<PathBuf, Error> {
        self.project
            .lock()
            .expect("the project mutex")
            .clone()
            .ok_or(Error::NoProject)
    }
}

/// Everything a command can refuse with.
///
/// One type for the whole surface rather than one per command, because the
/// frontend handles them in one place: a `catch` around `invoke`. The code is
/// what it branches on and the message is what it shows - see
/// [`vcw_contract::command::Failure`], which is this type as it crosses.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The engine refused or could not be reached.
    #[error("{0}")]
    Engine(#[from] vcw_core::engine::Error),
    /// The project would not open, or a row would not read.
    #[error("{0}")]
    Project(#[from] vcw_project::Error),
    /// Playback would not open, or the device went away.
    #[error("{0}")]
    Playback(#[from] vcw_core::playback::Error),
    /// The export refused.
    #[error("{0}")]
    Export(#[from] vcw_export::Error),
    /// A provider refused, was unreachable, or had no credential.
    ///
    /// Distinct from [`Self::Invalid`] on purpose: a query the shell would not
    /// send is the operator's to fix, and a provider that timed out is not.
    /// §40's offline mode arrives here too, as
    /// [`vcw_metadata::Error::Offline`].
    #[error("{0}")]
    Metadata(#[from] vcw_metadata::Error),
    /// A command argument was not usable.
    #[error("{field}: {why}")]
    Invalid {
        /// Which argument.
        field: String,
        /// What was wrong with it.
        why: String,
    },
    /// Nothing is armed, and this command needs a device.
    #[error("nothing is armed - arm a device and a project first")]
    NotArmed,
    /// Nothing is playing, and this command needs something to act on.
    #[error("nothing is playing")]
    NotPlaying,
    /// No project is open.
    #[error("no project is open")]
    NoProject,
}

impl Error {
    /// The stable slug a frontend branches on.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Engine(_) => "engine",
            Self::Project(_) => "project",
            Self::Playback(_) => "playback",
            Self::Export(_) => "export",
            Self::Metadata(_) => "metadata",
            Self::Invalid { .. } => "invalid-argument",
            Self::NotArmed => "not-armed",
            Self::NotPlaying => "not-playing",
            Self::NoProject => "no-project",
        }
    }

    /// The argument at fault, where one is.
    pub fn field(&self) -> Option<String> {
        match self {
            Self::Invalid { field, .. } => Some(field.clone()),
            _ => None,
        }
    }
}

impl From<vcw_contract::command::Invalid> for Error {
    fn from(invalid: vcw_contract::command::Invalid) -> Self {
        Self::Invalid {
            field: invalid.field.to_owned(),
            why: invalid.why,
        }
    }
}

impl Serialize for Error {
    /// Crosses as a [`Failure`], which is the type the generated TypeScript has.
    ///
    /// Tauri serializes a command's error with serde, so this is the only place
    /// the shape is decided. Going through `Failure` rather than deriving
    /// something ad hoc means the frontend's type is generated from the same
    /// declaration as everything else.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Failure {
            code: self.code().to_owned(),
            message: self.to_string(),
            field: self.field(),
        }
        .serialize(serializer)
    }
}
