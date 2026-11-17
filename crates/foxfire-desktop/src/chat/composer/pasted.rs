use crate::temp_dir::TempDir;
use foxfire_core::composer::attachment::PromptAttachment;
use std::fs;

#[derive(Default)]
pub(crate) struct PastedImages {
    dir: TempDir,
    saved: usize,
}

impl PastedImages {
    pub(crate) fn save(&mut self, png: &[u8]) -> Result<PromptAttachment, String> {
        self.saved += 1;
        let path = self.dir.path().join(format!("pasted-image-{}.png", self.saved));
        fs::create_dir_all(self.dir.path())
            .and_then(|()| fs::write(&path, png))
            .map_err(|error| format!("Couldn't save the pasted image: {error}"))?;
        Ok(PromptAttachment::new(path, self.dir.path()))
    }
}
