use traits;
use vfat::{File, Dir, Metadata, Cluster};

// TODO: You may need to change this definition.
#[derive(Debug)]
pub enum Entry {
    File(File),
    Dir(Dir)
}

// TODO: Implement any useful helper methods on `Entry`.

// FIXME: Implement `traits::Entry` for `Entry`.

impl traits::Entry for Entry {
    type File = File;
    type Dir = Dir;
    type Metadata = Metadata;

    fn name(&self) -> &str {
        match *self {
            Entry::File(ref f) => f.name.as_str(),
            Entry::Dir(ref d) => d.name.as_str(),
        }
    }

    fn metadata(&self) -> &Self::Metadata {
        match *self {
            Entry::File(ref f) => &f.metadata,
            Entry::Dir(ref d) => &d.metadata,
        }
    }

    fn as_file(&self) -> Option<&Self::File> {
        if let Entry::File(ref file) = *self {
            Some(file)
        } else {
            None
        }
    }

    fn as_dir(&self) -> Option<&Self::Dir> {
        if let Entry::Dir(ref dir) = *self {
            Some(dir)
        } else {
            None
        }
    }

    fn into_file(self) -> Option<Self::File> {
        if let Entry::File(file) = self {
            Some(file)
        } else {
            None
        }
    }

    fn into_dir(self) -> Option<Self::Dir> {
        if let Entry::Dir(dir) = self {
            Some(dir)
        } else {
            None
        }
    }
}