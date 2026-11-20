use crate::platform::{PathPrompt, Platform};
use crate::temp_dir::TempDir;
use foxfire_core::composer::attachment::{AttachedFile, PromptAttachment};
use foxfire_core::native::classify_paths;
use foxfire_ui::platform::{ChatPlatform, PickFiles};
use rouge::core::ImageSource;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Default)]
struct PastedImages {
    dir: TempDir,
    saved: usize,
}

impl PastedImages {
    fn save(&mut self, png: &[u8]) -> Result<PromptAttachment, String> {
        self.saved += 1;
        let path = self.dir.path().join(format!("pasted-image-{}.png", self.saved));
        fs::create_dir_all(self.dir.path())
            .and_then(|()| fs::write(&path, png))
            .map_err(|error| format!("Couldn't save the pasted image: {error}"))?;
        Ok(PromptAttachment::new(path, self.dir.path()))
    }
}

/// Adapts native dialogs and clipboard files to the shared conversation view.
pub(crate) struct NativeChatPlatform {
    platform: Rc<dyn Platform>,
    pasted: RefCell<PastedImages>,
}

impl NativeChatPlatform {
    pub(crate) fn new(platform: Rc<dyn Platform>) -> Self {
        Self { platform, pasted: RefCell::default() }
    }
}

impl ChatPlatform for NativeChatPlatform {
    fn open(&self, target: &str) {
        self.platform.open(target);
    }

    fn can_attach_local_files(&self) -> bool {
        true
    }

    fn classify_local_paths(&self, paths: &[PathBuf], root: &Path) -> Vec<AttachedFile> {
        classify_paths(paths.iter().cloned(), root)
    }

    fn pick_files(&self) -> PickFiles {
        self.platform.pick_paths(PathPrompt { title: "Attach".to_owned(), directories: false, multiple: true })
    }

    fn save_pasted_image(&self, png: &[u8]) -> Result<PromptAttachment, String> {
        self.pasted.borrow_mut().save(png)
    }

    fn attachment_image(&self, path: &Path) -> Option<ImageSource> {
        Some(ImageSource::File(path.to_path_buf()))
    }
}
