//! Layout and hit-testing for the job list's state column, shared by painting and click handling.

use pastegrab_app::board::Row;
use pastegrab_domain::job::JobState;

/// A rectangle in list-view client coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

impl Rect {
    pub(crate) const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// Scales a 96-DPI pixel measurement to `dpi`.
pub(crate) const fn scale(value: i32, dpi: i32) -> i32 {
    value * dpi / 96
}

/// An inline action clicked inside a row's state cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowAction {
    Cancel,
    Open,
    Folder,
}

/// Parts of a cancellable row's state cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CancelLayout {
    pub(crate) bar: Option<Rect>,
    pub(crate) label: Rect,
    pub(crate) cancel: Rect,
}

/// Parts of a finished row's state cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DoneLayout {
    pub(crate) label: Rect,
    pub(crate) buttons: Option<(Rect, Rect)>,
}

/// Places the optional progress bar, state label and cancel glyph inside `cell`.
pub(crate) fn cancel_layout(cell: Rect, dpi: i32, has_bar: bool) -> CancelLayout {
    let pad = scale(6, dpi);
    let cancel_w = scale(24, dpi);
    let inset = scale(2, dpi);
    let cancel = Rect {
        left: (cell.right - pad - cancel_w).max(cell.left),
        top: cell.top + inset,
        right: (cell.right - pad).max(cell.left),
        bottom: (cell.bottom - inset).max(cell.top + inset),
    };
    let bar = has_bar.then(|| {
        let bar_inset = scale(5, dpi);
        let avail = (cancel.left - cell.left).max(0);
        let width = scale(96, dpi).min(avail / 3);
        Rect {
            left: cell.left + pad,
            top: cell.top + bar_inset,
            right: cell.left + pad + width,
            bottom: (cell.bottom - bar_inset).max(cell.top + bar_inset),
        }
    });
    let label_left = bar.map_or(cell.left + pad, |b| b.right + pad);
    let label = Rect {
        left: label_left,
        top: cell.top,
        right: (cancel.left - pad).max(label_left),
        bottom: cell.bottom,
    };
    CancelLayout { bar, label, cancel }
}

/// Places the finished label and its «Открыть» and «Папка» buttons inside `cell`.
pub(crate) fn done_layout(cell: Rect, dpi: i32, open_w: i32, folder_w: i32) -> DoneLayout {
    let pad = scale(6, dpi);
    let btn_pad = scale(8, dpi);
    let btn_gap = scale(6, dpi);
    let inset = scale(3, dpi);
    let folder_right = cell.right - pad;
    let folder_left = folder_right - (folder_w + 2 * btn_pad);
    let open_right = folder_left - btn_gap;
    let open_left = open_right - (open_w + 2 * btn_pad);
    let min_label = scale(56, dpi);
    if open_left - pad - (cell.left + pad) >= min_label {
        let open = Rect {
            left: open_left,
            top: cell.top + inset,
            right: open_right,
            bottom: (cell.bottom - inset).max(cell.top + inset),
        };
        let folder = Rect {
            left: folder_left,
            top: cell.top + inset,
            right: folder_right,
            bottom: (cell.bottom - inset).max(cell.top + inset),
        };
        let label = Rect {
            left: cell.left + pad,
            top: cell.top,
            right: open_left - pad,
            bottom: cell.bottom,
        };
        DoneLayout {
            label,
            buttons: Some((open, folder)),
        }
    } else {
        DoneLayout {
            label: Rect {
                left: cell.left + pad,
                top: cell.top,
                right: (cell.right - pad).max(cell.left + pad),
                bottom: cell.bottom,
            },
            buttons: None,
        }
    }
}

/// Which inline action lies under `(x, y)` in `cell`, if any.
pub(crate) fn hit(
    row: &Row,
    cell: Rect,
    x: i32,
    y: i32,
    dpi: i32,
    open_w: i32,
    folder_w: i32,
) -> Option<RowAction> {
    if !row.state.is_over() {
        let parts = cancel_layout(cell, dpi, row.permille().is_some());
        return parts.cancel.contains(x, y).then_some(RowAction::Cancel);
    }
    if row.state == JobState::Done && row.file.is_some() {
        let (open, folder) = done_layout(cell, dpi, open_w, folder_w).buttons?;
        if open.contains(x, y) {
            return Some(RowAction::Open);
        }
        if folder.contains(x, y) {
            return Some(RowAction::Folder);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pastegrab_app::board::Row;
    use pastegrab_app::ports::{Phase, Progress};
    use pastegrab_domain::job::{FailureKind, JobId, JobState};
    use pastegrab_domain::profile::ProfileKind;

    use super::{Rect, RowAction, cancel_layout, done_layout, hit};

    fn row(state: JobState) -> Row {
        Row {
            id: JobId(1),
            url: "https://youtu.be/AbC123xyz_0".into(),
            profile: ProfileKind::Video,
            title: Some("Забег".into()),
            duration: Some(30),
            state,
            plan: None,
            no_plan: None,
            progress: None,
            file: None,
            failure: None,
        }
    }

    #[test]
    fn a_running_row_places_cancel_at_the_right_without_overlap() {
        let cell = Rect {
            left: 360,
            top: 28,
            right: 720,
            bottom: 56,
        };
        let mut active = row(JobState::Fetching);
        active.progress = Some((
            Phase::Download,
            Progress {
                done: 50,
                total: Some(100),
                speed: Some(1_000_000),
                eta: None,
            },
        ));
        let layout = cancel_layout(cell, 96, true);
        let bar = layout.bar.expect("progress bar");
        assert!(bar.right < layout.label.left);
        assert!(layout.label.right < layout.cancel.left);
        assert!(layout.cancel.right < cell.right);
        let cx = i32::midpoint(layout.cancel.left, layout.cancel.right);
        let cy = i32::midpoint(layout.cancel.top, layout.cancel.bottom);
        assert_eq!(
            hit(&active, cell, cx, cy, 96, 60, 50),
            Some(RowAction::Cancel)
        );
        assert_eq!(hit(&active, cell, bar.left + 2, cy, 96, 60, 50), None);
    }

    #[test]
    fn a_finished_row_hits_open_and_folder_and_drops_them_when_too_narrow() {
        let cell = Rect {
            left: 360,
            top: 0,
            right: 720,
            bottom: 28,
        };
        let mut done = row(JobState::Done);
        done.file = Some(PathBuf::from("C:\\Videos\\a.mp4"));
        let layout = done_layout(cell, 96, 64, 48);
        let (open, folder) = layout.buttons.expect("buttons fit");
        assert!(layout.label.right < open.left);
        assert!(open.right < folder.left);
        assert!(folder.right < cell.right);
        let mid_y = 14;
        assert_eq!(
            hit(
                &done,
                cell,
                i32::midpoint(open.left, open.right),
                mid_y,
                96,
                64,
                48
            ),
            Some(RowAction::Open)
        );
        assert_eq!(
            hit(
                &done,
                cell,
                i32::midpoint(folder.left, folder.right),
                mid_y,
                96,
                64,
                48
            ),
            Some(RowAction::Folder)
        );
        assert_eq!(hit(&done, cell, cell.left + 10, mid_y, 96, 64, 48), None);

        let narrow = Rect {
            left: 0,
            top: 0,
            right: 120,
            bottom: 28,
        };
        assert_eq!(done_layout(narrow, 96, 64, 48).buttons, None);
        assert_eq!(hit(&done, narrow, 100, mid_y, 96, 64, 48), None);

        let failed = row(JobState::Failed(FailureKind::Network));
        assert_eq!(hit(&failed, cell, 700, mid_y, 96, 64, 48), None);
    }
}
