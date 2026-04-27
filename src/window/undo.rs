pub enum UndoOp {
    Rename {
        file: gio::File,
        old_name: String,
        new_name: String,
    },
    NewFolder {
        dir: gio::File,
    },
}

impl std::fmt::Debug for UndoOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UndoOp::Rename { old_name, new_name, .. } => {
                write!(f, "Rename({:?} -> {:?})", old_name, new_name)
            }
            UndoOp::NewFolder { .. } => write!(f, "NewFolder"),
        }
    }
}
