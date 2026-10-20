//! Editor tab model (text / image / side-by-side diff).
use super::*;

pub(super) struct EditorTab {
    pub(super) path: PathBuf,
    pub(super) body: TabBody,
    pub(super) is_modified: bool,
    /// VS Code–style preview: replaced by the next single-click open until pinned.
    pub(super) preview: bool,
    /// Default folder when saving an in-memory tab (workspace New File).
    pub(super) untitled_save_dir: Option<PathBuf>,
}

pub(super) enum TabBody {
    Text {
        language: Language,
        state: Entity<EditorState>,
        /// Rendered view (Markdown → TextView, SVG → img) instead of the source editor.
        rendered_preview: bool,
        _subscriptions: Vec<Subscription>,
    },
    /// Read-only side-by-side git diff (equal panes).
    Diff {
        language: Language,
        kind: DiffKind,
        old: Entity<EditorState>,
        new: Entity<EditorState>,
        /// Keeps line-fill highlights alive for the tab lifetime.
        _highlights: (RangeDecorationCollection, RangeDecorationCollection),
        _subscriptions: Vec<Subscription>,
    },
    Image,
}

/// What the two panes of a [`TabBody::Diff`] represent.
#[derive(Clone)]
pub(super) enum DiffKind {
    /// HEAD | Working Tree
    Change,
    /// Current (ours) | Incoming (theirs)
    Conflict {
        repo: PathBuf,
        rel_path: String,
        /// Per-hunk choice (`None` = still open). Length matches `conflict_hunks`.
        hunk_choices: Vec<Option<ConflictSide>>,
    },
}

pub(super) fn conflict_side_to_hunk(side: ConflictSide) -> HunkSide {
    match side {
        ConflictSide::Current => HunkSide::Old,
        ConflictSide::Incoming => HunkSide::New,
    }
}

impl DiffKind {
    pub(super) fn left_title(&self) -> &'static str {
        match self {
            Self::Change => "HEAD",
            Self::Conflict { .. } => "Current",
        }
    }

    pub(super) fn right_title(&self) -> &'static str {
        match self {
            Self::Change => "Working Tree",
            Self::Conflict { .. } => "Incoming",
        }
    }
}

impl EditorTab {
    pub(super) fn label(&self) -> String {
        document::tab_label(&self.path)
    }

    pub(super) fn language(&self) -> Option<Language> {
        match &self.body {
            TabBody::Text { language, .. } | TabBody::Diff { language, .. } => Some(*language),
            TabBody::Image => None,
        }
    }

    pub(super) fn is_image(&self) -> bool {
        matches!(self.body, TabBody::Image)
    }

    pub(super) fn is_diff(&self) -> bool {
        matches!(self.body, TabBody::Diff { .. })
    }

    pub(super) fn editor_state(&self) -> Option<&Entity<EditorState>> {
        match &self.body {
            TabBody::Text { state, .. } => Some(state),
            TabBody::Diff { new, .. } => Some(new),
            TabBody::Image => None,
        }
    }

    pub(super) fn rendered_preview(&self) -> bool {
        match &self.body {
            TabBody::Text {
                rendered_preview, ..
            } => *rendered_preview,
            TabBody::Diff { .. } | TabBody::Image => false,
        }
    }

    pub(super) fn supports_rendered_preview(&self) -> bool {
        matches!(self.body, TabBody::Text { .. })
            && self
                .language()
                .is_some_and(Language::supports_rendered_preview)
    }
}

