pub enum UndoOp {
    Rename {
        file: gio::File,
        old_name: String,
        new_name: String,
    },
    NewFolder {
        dir: gio::File,
    },
    /// Move-to-trash. We remember every file's original URI so undo
    /// can locate the corresponding trash entry by `trash::orig-path`
    /// and move it back. Holding the original `gio::File` itself is
    /// not enough — after trash, that file no longer exists.
    Trash {
        originals: Vec<gio::File>,
    },
}

impl std::fmt::Debug for UndoOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UndoOp::Rename { old_name, new_name, .. } => {
                write!(f, "Rename({:?} -> {:?})", old_name, new_name)
            }
            UndoOp::NewFolder { .. } => write!(f, "NewFolder"),
            UndoOp::Trash { originals } => write!(f, "Trash(n={})", originals.len()),
        }
    }
}
