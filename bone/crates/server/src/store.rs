use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

pub struct Blobs {
    dir: PathBuf,
}

impl Blobs {
    pub fn open(data_dir: &Path) -> Result<Self> {
        let dir = data_dir.join("blobs");
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        Ok(Self { dir })
    }

    pub fn put(&self, data: &[u8]) -> Result<String> {
        let id = hex::encode(Sha256::digest(data));
        let path = self.path(&id);
        if path.exists() {
            return Ok(id);
        }
        let parent = path.parent().expect("blob paths have a parent");
        std::fs::create_dir_all(parent)?;
        let tmp = parent.join(format!(".{id}.tmp"));
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        std::fs::rename(&tmp, &path)?;
        Ok(id)
    }

    pub fn get(&self, id: &str) -> Result<Vec<u8>> {
        Ok(std::fs::read(self.path(id))?)
    }

    // blobs newer than min_age may belong to a fetch whose row isn't updated yet
    pub fn sweep(&self, keep: &HashSet<String>, min_age: Duration) -> Result<usize> {
        let cutoff = SystemTime::now() - min_age;
        let mut removed = 0;
        for shard in std::fs::read_dir(&self.dir)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(shard.path())? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if keep.contains(&name) || entry.metadata()?.modified()? > cutoff {
                    continue;
                }
                std::fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(&id[..2]).join(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        crate::testutil::temp_dir("blobs")
    }

    #[test]
    fn put_is_content_addressed() {
        let dir = temp();
        let blobs = Blobs::open(&dir).unwrap();
        let a = blobs.put(b"hello").unwrap();
        let b = blobs.put(b"hello").unwrap();
        assert_eq!(a, b);
        assert_eq!(blobs.get(&a).unwrap(), b"hello");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sweep_keeps_referenced_and_recent() {
        let dir = temp();
        let blobs = Blobs::open(&dir).unwrap();
        let kept = blobs.put(b"kept").unwrap();
        let orphan = blobs.put(b"orphan").unwrap();
        let keep = HashSet::from([kept.clone()]);
        assert_eq!(blobs.sweep(&keep, Duration::from_secs(3600)).unwrap(), 0);
        assert_eq!(blobs.sweep(&keep, Duration::ZERO).unwrap(), 1);
        assert!(blobs.get(&kept).is_ok());
        assert!(blobs.get(&orphan).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
