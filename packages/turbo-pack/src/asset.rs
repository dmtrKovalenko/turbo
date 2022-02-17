use turbo_tasks_fs::{FileContentRef, FileSystemPathRef};

#[turbo_tasks::value]
#[derive(Hash, PartialEq, Eq)]
pub struct AssetsSet {
    pub assets: Vec<AssetRef>,
}

#[turbo_tasks::value_trait]
pub trait Asset {
    fn path(&self) -> FileSystemPathRef;
    fn content(&self) -> FileContentRef;
    fn references(&self) -> AssetsSetRef;
}