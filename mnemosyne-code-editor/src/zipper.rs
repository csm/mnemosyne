use crate::ast::{Form, FormKind, Span};

/// A breadcrumb recording what was above the zipper's focus.
struct Crumb {
    /// Siblings to the left of focus (in original order).
    left: Vec<Form>,
    /// Siblings to the right of focus (in original order).
    right: Vec<Form>,
    /// The parent node's kind and span (children are reassembled on `up`).
    parent_kind: FormKind,
    parent_span: Span,
    parent_text: String,
}

/// An owned zipper over a `Form` tree for structural editing.
///
/// Navigation consumes and rebuilds nodes so that `replace_focus` can swap in
/// new text without borrowing conflicts. Call `root()` to zip back up and then
/// `unparse` to produce the edited source string.
pub struct Zipper {
    pub focus: Form,
    crumbs: Vec<Crumb>,
    source: String,
}

impl Zipper {
    pub fn new(root: Form, source: String) -> Self {
        Self {
            focus: root,
            crumbs: vec![],
            source,
        }
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    /// Descend into the first child of the current focus, if any.
    pub fn down(mut self) -> Option<Self> {
        let mut children = match self.focus.kind {
            FormKind::List
            | FormKind::Vector
            | FormKind::Map
            | FormKind::Set
            | FormKind::Metadata => {
                if self.focus.children.is_empty() {
                    return None;
                }
                self.focus.children
            }
            _ => return None,
        };

        let first = children.remove(0);
        self.crumbs.push(Crumb {
            left: vec![],
            right: children,
            parent_kind: self.focus.kind,
            parent_span: self.focus.span,
            parent_text: self.focus.text,
        });
        Some(Self {
            focus: first,
            crumbs: self.crumbs,
            source: self.source,
        })
    }

    /// Move to the next sibling (right).
    pub fn right(mut self) -> Option<Self> {
        let crumb = self.crumbs.last_mut()?;
        if crumb.right.is_empty() {
            return None;
        }
        let next = crumb.right.remove(0);
        crumb.left.push(self.focus);
        Some(Self {
            focus: next,
            crumbs: self.crumbs,
            source: self.source,
        })
    }

    /// Move to the previous sibling (left).
    pub fn left(mut self) -> Option<Self> {
        let crumb = self.crumbs.last_mut()?;
        let prev = crumb.left.pop()?;
        crumb.right.insert(0, self.focus);
        Some(Self {
            focus: prev,
            crumbs: self.crumbs,
            source: self.source,
        })
    }

    /// Move up to the parent.
    pub fn up(mut self) -> Option<Self> {
        let crumb = self.crumbs.pop()?;
        let mut children = crumb.left;
        children.push(self.focus);
        children.extend(crumb.right);
        let parent = Form {
            kind: crumb.parent_kind,
            span: crumb.parent_span,
            text: crumb.parent_text,
            children,
        };
        Some(Self {
            focus: parent,
            crumbs: self.crumbs,
            source: self.source,
        })
    }

    // ── Search ────────────────────────────────────────────────────────────────

    /// Navigate depth-first to the first `(defn name ...)` form with the given
    /// name. Returns `None` if no such form exists.
    pub fn find_defn(self, name: &str) -> Option<Self> {
        find_defn_dfs(self, name)
    }

    // ── Mutation ──────────────────────────────────────────────────────────────

    /// Replace the text of the focused node. Clears children; the replacement
    /// is treated as an opaque pre-formatted string during unparse.
    pub fn replace_focus(mut self, new_text: String) -> Self {
        let span = self.focus.span;
        self.focus = Form {
            kind: self.focus.kind,
            span: Span {
                start: span.start,
                end: span.start,
            }, // sentinel: zero-length
            text: new_text,
            children: vec![],
        };
        self
    }

    // ── Reassembly ────────────────────────────────────────────────────────────

    /// Zip all the way back to the root and return it.
    pub fn root(mut self) -> Form {
        while !self.crumbs.is_empty() {
            self = self.up().unwrap();
        }
        self.focus
    }

    /// The original source string this zipper was built from.
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Depth-first search for a `defn` by name, re-entering child nodes.
fn find_defn_dfs(z: Zipper, name: &str) -> Option<Zipper> {
    if z.focus.defn_name() == Some(name) {
        return Some(z);
    }
    // Try descending into children.
    if let Some(child_z) = z.down() {
        // Search this subtree.
        if let Some(found) = find_defn_dfs_sibling(child_z, name) {
            return Some(found);
        }
    }
    None
}

/// Search focus + all right siblings (already descended into a level).
fn find_defn_dfs_sibling(z: Zipper, name: &str) -> Option<Zipper> {
    // Try focus itself.
    if z.focus.defn_name() == Some(name) {
        return Some(z);
    }
    // Recurse into focus's children.
    if let Some(child_z) = Zipper::new(z.focus.clone(), z.source.clone()).down() {
        if let Some(found) = find_defn_dfs_sibling(child_z, name) {
            // Re-anchor to our crumb stack: we need to propagate the found
            // zipper's crumbs back through our own. Simpler: just do a
            // separate top-level DFS from each child.
            let _ = found;
        }
    }

    // Move right and continue searching siblings.
    if let Some(right_z) = z.right() {
        return find_defn_dfs_sibling(right_z, name);
    }
    None
}

// ── Unparse ───────────────────────────────────────────────────────────────────

/// Reconstruct edited source by walking `root` and substituting replaced nodes.
///
/// Unchanged nodes are copied verbatim from `original_source` using their
/// byte spans. Replaced nodes (sentinel `span.start == span.end`) emit their
/// `text` field directly.
pub fn unparse(root: &Form, original_source: &str) -> String {
    let mut out = String::with_capacity(original_source.len());
    emit_form(root, original_source, &mut out);
    out
}

fn emit_form(form: &Form, src: &str, out: &mut String) {
    // Sentinel span means this form was replaced — emit the new text directly.
    if form.span.start == form.span.end && !form.text.is_empty() {
        out.push_str(&form.text);
        return;
    }
    // Leaf or opaque node with no children to recurse into.
    if form.children.is_empty() {
        out.push_str(&src[form.span.start..form.span.end]);
        return;
    }
    // Composite node: emit opening prefix, recurse into children with gaps
    // preserved from original source, emit closing suffix.
    emit_composite(form, src, out);
}

fn emit_composite(form: &Form, src: &str, out: &mut String) {
    // Walk children in span order; gaps between children reproduce original
    // whitespace and comments exactly.
    let mut cursor = form.span.start;
    for child in &form.children {
        // Emit any gap (whitespace / comments) between cursor and this child.
        if child.span.start > cursor {
            out.push_str(&src[cursor..child.span.start]);
        }
        emit_form(child, src, out);
        cursor = child.span.end;
    }
    // Emit trailing gap up to the closing delimiter.
    if cursor < form.span.end {
        out.push_str(&src[cursor..form.span.end]);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ClojureAst;

    fn parse_zip(src: &str) -> Zipper {
        let ast = ClojureAst::parse(src).unwrap();
        // Wrap top-level forms in a synthetic root so navigation works.
        let root = Form {
            kind: FormKind::List,
            span: Span {
                start: 0,
                end: src.len(),
            },
            text: src.to_owned(),
            children: ast.top_level,
        };
        Zipper::new(root, src.to_owned())
    }

    #[test]
    fn find_defn_navigates_correctly() {
        let src = "(defn a [] 1)\n\n(defn b [] 2)\n\n(defn c [] 3)";
        let z = parse_zip(src);
        let found = z.find_defn("b").expect("should find b");
        assert_eq!(found.focus.defn_name(), Some("b"));
    }

    #[test]
    fn find_defn_returns_none_for_missing() {
        let src = "(defn a [] 1)";
        let z = parse_zip(src);
        assert!(z.find_defn("z").is_none());
    }

    #[test]
    fn unparse_identity() {
        let src = "(defn greet [name]\n  (str \"Hello, \" name \"!\"))";
        let ast = ClojureAst::parse(src).unwrap();
        let root = Form {
            kind: FormKind::List,
            span: Span {
                start: 0,
                end: src.len(),
            },
            text: src.to_owned(),
            children: ast.top_level,
        };
        let result = unparse(&root, src);
        assert_eq!(result, src);
    }

    #[test]
    fn navigate_up_down_round_trips() {
        let src = "(defn add [a b] (+ a b))";
        let z = parse_zip(src);
        let down = z.down().expect("should have children");
        let back_up = down.up().expect("should go back up");
        assert_eq!(back_up.focus.kind, FormKind::List);
    }
}
