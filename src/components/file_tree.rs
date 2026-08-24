use crate::theme;
use iocraft::prelude::*;
use std::path::{Path, PathBuf};

/// A node in the sidebar file tree: a file or directory on disk.
#[derive(Clone, Debug)]
pub struct FileNode {
    /// Display name (file/folder name only).
    pub name: String,
    /// Full path of this entry.
    pub path: PathBuf,
    pub is_dir: bool,
    /// Directories only: whether the folder is expanded in the tree.
    pub expanded: bool,
    /// Directories only: lazily-loaded children (empty until expanded once,
    /// or genuinely empty folders).
    pub children: Vec<FileNode>,
}

impl FileNode {
    /// Builds the root node for `dir`, listing one level of entries.
    pub fn from_dir(dir: &Path) -> Option<FileNode> {
        Some(FileNode {
            name: dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| dir.display().to_string()),
            path: dir.to_path_buf(),
            is_dir: true,
            expanded: true,
            children: read_dir_sorted(dir)?,
        })
    }

    /// Loads (or refreshes) this directory's children, keeping existing
    /// expansion state of previously-known subdirectories.
    pub fn reload_children(&mut self) {
        if !self.is_dir {
            return;
        }
        let mut fresh = read_dir_sorted(&self.path).unwrap_or_default();
        // Preserve expansion flags of known children.
        for child in &mut fresh {
            if let Some(old) = self.children.iter().find(|c| c.path == child.path) {
                child.expanded = old.expanded;
                child.children = old.children.clone();
            }
        }
        self.children = fresh;
        self.expanded = true;
    }

    /// Depth-first listing of currently *visible* nodes (expanded dirs walk
    /// into their children, collapsed dirs stay as single rows).
    pub fn visible_nodes<'a>(node: &'a FileNode, out: &mut Vec<&'a FileNode>) {
        out.push(node);
        if node.is_dir && node.expanded {
            for child in &node.children {
                Self::visible_nodes(child, out);
            }
        }
    }

    /// Toggles expansion of the directory at `target` (matching by path),
    /// lazily loading children the first time it is expanded.
    ///
    /// Returns `true` if some directory's expansion changed.
    pub fn toggle(node: &mut FileNode, target: &Path) -> bool {
        if node.path == target && node.is_dir {
            if !node.expanded && node.children.is_empty() {
                node.reload_children();
            } else {
                node.expanded = !node.expanded;
            }
            return true;
        }
        if node.is_dir && target.starts_with(&node.path) {
            for child in &mut node.children {
                if FileNode::toggle(child, target) {
                    return true;
                }
            }
        }
        false
    }
}

/// Lists `dir` sorted directories-first then alphabetically, skipping hidden
/// entries and common build/VCS noise.
fn read_dir_sorted(dir: &Path) -> Option<Vec<FileNode>> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut nodes: Vec<(bool, String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return None;
            }
            let ft = e.file_type().ok()?;
            let is_dir = ft.is_dir() || ft.is_symlink() && e.path().is_dir();
            if !is_dir && is_noise(&name) {
                return None;
            }
            if is_dir && is_noise_dir(&name) {
                return None;
            }
            Some((is_dir, name, e.path()))
        })
        .collect();
    nodes.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Some(
        nodes
            .into_iter()
            .map(|(is_dir, name, path)| FileNode {
                name,
                path,
                is_dir,
                expanded: false,
                children: Vec::new(),
            })
            .collect(),
    )
}

fn is_noise(name: &str) -> bool {
    matches!(
        name,
        "node_modules" | "target" | "dist" | "build" | "__pycache__" | ".DS_Store"
    )
}

fn is_noise_dir(name: &str) -> bool {
    matches!(name, "node_modules" | "target" | "build")
}

/// Properties for the [`FileTreePanel`] component.
#[derive(Default, Props)]
pub struct FileTreePanelProps {
    /// Flattened list of currently visible nodes (rendered top to bottom).
    pub visible: Vec<FileNode>,
    /// Index of the highlighted row within `visible`.
    pub selected: usize,
    /// Panel width in columns.
    pub width: u32,
}

/// Collapsible left-side file explorer: folder tree with ▸/▾ indicators and a
/// highlighted selected row.
#[component]
pub fn FileTreePanel(props: &FileTreePanelProps) -> impl Into<AnyElement<'static>> {
    element! {
        View(
            width: props.width,
            height: 100pct,
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Single,
            border_color: theme::border(),
            background_color: theme::dark_bg(),
        ) {
            View(background_color: theme::status_bg(), padding_left: 1) {
                Text(content: " Files ", color: theme::fg(), weight: Weight::Bold)
            }
            View(flex_grow: 1.0, flex_direction: FlexDirection::Column) {
                #(props.visible.iter().enumerate().map(|(i, node)| {
                    let selected = i == props.selected;
                    let indent = "  ".repeat(depth_of(&props.visible, i));
                    let icon = if node.is_dir {
                        if node.expanded { "▾ " } else { "▸ " }
                    } else {
                        "  "
                    };
                    element! {
                        View(height: 1, background_color: selected.then_some(theme::status_bg())) {
                            Text(
                                content: format!("{indent}{icon}{}", node.name),
                                color: if node.is_dir { theme::blue() } else { theme::fg() },
                                weight: if selected || node.is_dir { Weight::Bold } else { Weight::Normal },
                            )
                        }
                    }.into_any()
                }))
            }
            View(height: 1, border_style: BorderStyle::Single, border_edges: Edges::Top, border_color: theme::border(), padding_left: 1) {
                Text(content: "↑↓ move · ↵ open · q close", color: theme::comment())
            }
        }
    }
    .into_any()
}

/// Depth of a visible row = number of ancestor directories above it.
fn depth_of(visible: &[FileNode], idx: usize) -> usize {
    // Depth from path nesting relative to the root entry.
    let root_components = visible[0].path.components().count();
    visible[idx]
        .path
        .components()
        .count()
        .saturating_sub(root_components)
        .saturating_sub(1)
}
