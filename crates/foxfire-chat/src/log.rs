use crate::error::ChatError;
use foxfire_acp::chat::ChatChannelEvent;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub(crate) struct Log {
    file: File,
    length: u64,
}

impl Log {
    pub(crate) fn open(path: &Path) -> Result<(Self, Vec<ChatChannelEvent>), ChatError> {
        let mut file = File::options().read(true).write(true).create(true).truncate(false).open(path)?;
        file.try_lock()?;
        let mut stored = Vec::new();
        file.read_to_end(&mut stored)?;
        let length = stored.iter().rposition(|byte| *byte == b'\n').map_or(0, |end| end + 1);
        let events = serde_json::Deserializer::from_slice(&stored[..length]).into_iter().collect::<Result<_, _>>()?;
        Ok((Self { file, length: length as u64 }, events))
    }

    pub(crate) fn append(&mut self, events: &[ChatChannelEvent]) -> Result<(), ChatError> {
        let mut lines = Vec::new();
        for event in events {
            serde_json::to_writer(&mut lines, event)?;
            lines.push(b'\n');
        }
        self.file.set_len(self.length)?;
        self.file.seek(SeekFrom::Start(self.length))?;
        self.file.write_all(&lines)?;
        self.length += lines.len() as u64;
        Ok(())
    }
}
