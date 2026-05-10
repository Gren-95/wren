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
    /// Cut+paste move or drag-drop move of one or more files. Each
    /// entry is `(from, to)` as the move occurred — undo runs `to → from`,
    /// redo runs `from → to`. One Ctrl+Z reverses the whole batch.
    MoveBatch {
        moves: Vec<(gio::File, gio::File)>,
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
            UndoOp::MoveBatch { moves } => write!(f, "MoveBatch(n={})", moves.len()),
        }
    }
}
