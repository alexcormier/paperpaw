//! Startup snapshots: count physical reads and exercise installation recovery.

use super::*;
use alloc::rc::Rc;
use core::cell::Cell;
use paperpaw_platform::{DirEntry, StorageError};
use paperpaw_sdk::{MemoryStorage, Version};

const INSTALLED: &str = ".paperpaw/plugin-code.txt";

struct Counted {
    inner: MemoryStorage,
    reads: Rc<Cell<usize>>,
}

impl Storage for Counted {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if path == INSTALLED {
            self.reads.set(self.reads.get() + 1);
        }
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, data)
    }
    fn list(&self, path: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(path, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
}

struct Code(Id);
impl Plugin for Code {
    fn manifest(&self) -> Manifest {
        Manifest::new(self.0.clone(), "Code", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, _: &mut Registrar) -> Result<(), RegisterError> {
        Ok(())
    }
}

struct Loader(Rc<Cell<usize>>);
impl CodeLoader for Loader {
    fn load(
        &mut self,
        _: &dyn Storage,
        file: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        assert_eq!(file.sha256, [7; 32]);
        self.0.set(self.0.get() + 1);
        Ok(Box::new(Code(file.id.clone())))
    }
}

#[test]
fn thirty_two_plugins_read_the_installed_list_once() {
    let storage = MemoryStorage::new();
    for n in 0..32 {
        let id = format!("plugin-{n}");
        storage
            .write(
                &format!("plugins/{id}/plugin.ini"),
                b"[plugin]\nversion = 1.0.0\n",
            )
            .unwrap();
        storage
            .write(&format!("plugins/{id}/code.ppn"), b"PPNP")
            .unwrap();
        assert!(paperpaw_sdk::native::remember_code(&storage, &id, &[7; 32]));
    }
    // Beyond Recall's small-file threshold: this proves a startup snapshot,
    // not an accidental cache hit for a tiny fixture.
    storage
        .append(INSTALLED, "#".repeat(5000).as_bytes())
        .unwrap();
    let reads = Rc::new(Cell::new(0));
    let loaded = Rc::new(Cell::new(0));
    let kernel = Kernel::builder(crate::tests::DEVICE)
        .storage(Counted {
            inner: storage,
            reads: reads.clone(),
        })
        .code_plugins(Native::new("code.ppn"), Box::new(Loader(loaded.clone())))
        .build()
        .unwrap();
    assert_eq!(loaded.get(), 32);
    assert_eq!(reads.get(), 1);
    assert_eq!(
        kernel
            .plugins()
            .iter()
            .filter(|p| p.kind == PluginKind::Code && p.state == PluginState::Running)
            .count(),
        32
    );
}
