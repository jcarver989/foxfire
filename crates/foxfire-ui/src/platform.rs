use foxfire_core::composer::attachment::{AttachedFile, PromptAttachment};
use rouge::core::ImageSource;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

pub type PickFiles = Pin<Box<dyn Future<Output = Option<Vec<PathBuf>>>>>;

/// Platform capabilities needed by the shared conversation view.
pub trait ChatPlatform: 'static {
    fn open(&self, target: &str);

    fn can_attach_local_files(&self) -> bool {
        false
    }

    /// Classifies paths obtained from this host's picker, clipboard, or drag and drop.
    fn classify_local_paths(&self, _paths: &[PathBuf], _root: &Path) -> Vec<AttachedFile> {
        Vec::new()
    }

    fn pick_files(&self) -> PickFiles {
        Box::pin(async { None })
    }

    fn save_pasted_image(&self, _png: &[u8]) -> Result<PromptAttachment, String> {
        Err("Pasted images are not supported by this client".to_owned())
    }

    fn attachment_image(&self, _path: &Path) -> Option<ImageSource> {
        None
    }
}
