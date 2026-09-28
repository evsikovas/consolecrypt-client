//! Opening working copies in an external application.

use cc_platform_core::{AppRef, ChooseOutcome, FileOpener, OpenError};
use std::fmt;
use std::path::Path;

/// Which application opens a working copy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum OpenWith {
    /// The OS default application for the file type ("Open").
    #[default]
    Default,
    /// A specific application ("Open With…" → picked in the UI).
    App(AppRef),
    /// Show the platform's application chooser ("Open With…" → system
    /// dialog). The chosen application (when reported) is reused for later
    /// re-opens and remote copies of the session.
    Choose,
}

/// Opens working copies. Implemented for [`FileOpener`]; tests inject a
/// recording fake. Called on a blocking thread (the chooser blocks).
pub trait EditOpener: Send + Sync + fmt::Debug + 'static {
    fn open(&self, path: &Path, with: &OpenWith) -> Result<ChooseOutcome, OpenError>;
}

impl EditOpener for FileOpener {
    fn open(&self, path: &Path, with: &OpenWith) -> Result<ChooseOutcome, OpenError> {
        match with {
            OpenWith::Default => self
                .open_path(path)
                .map(|()| ChooseOutcome::Opened { app: None }),
            OpenWith::App(app) => self.open_with(path, app).map(|()| ChooseOutcome::Opened {
                app: Some(app.clone()),
            }),
            OpenWith::Choose => self.choose_app_and_open(path),
        }
    }
}
