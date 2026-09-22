//! Canonical place paths, projections, overlap, and expression paths.

use super::{
    fmt, Diagnostic, Expr, ExprKind, FunctionChecker, HashMap, LocalBinding, Result, Type,
};

/// The index or key that selected an element or entry place. Only two
/// literal selectors of one kind with different values are provably
/// disjoint; a selector known only at run time overlaps every selector of
/// its kind (ADR-0061, 2026-09-21 section, A3).
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum PlaceSelector {
    Int(i128),
    Str(String),
    Bool(bool),
    Dynamic,
}

impl PlaceSelector {
    /// Whether two selections of one collection are provably different
    /// slots.
    pub(super) fn disjoint_from(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(left), Self::Int(right)) => left != right,
            (Self::Str(left), Self::Str(right)) => left != right,
            (Self::Bool(left), Self::Bool(right)) => left != right,
            _ => false,
        }
    }
}

impl fmt::Display for PlaceSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(value) => write!(f, "{value}"),
            Self::Str(value) => write!(f, "{value:?}"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Dynamic => write!(f, "?"),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum PlaceProjection {
    Field(String),
    Tuple(usize),
    /// A list element selected by an index (ADR-0061, 2026-09-21 section).
    Element(PlaceSelector),
    /// A dictionary entry selected by a key (ADR-0061, 2026-09-21 section).
    Entry(PlaceSelector),
}

impl PlaceProjection {
    /// Whether two projections at the same depth name provably different
    /// places: distinct fields or tuple positions, or element/entry
    /// selections with different literal selectors.
    fn disjoint_from(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Field(left), Self::Field(right)) => left != right,
            (Self::Tuple(left), Self::Tuple(right)) => left != right,
            (Self::Element(left), Self::Element(right))
            | (Self::Entry(left), Self::Entry(right)) => left.disjoint_from(right),
            // A syntactic position spelled on a list or an integer-keyed
            // dictionary (`items.0` from the place builders) names that
            // element or entry.
            (Self::Tuple(position), Self::Element(selector) | Self::Entry(selector))
            | (Self::Element(selector) | Self::Entry(selector), Self::Tuple(position)) => {
                PlaceSelector::Int(*position as i128).disjoint_from(selector)
            }
            _ => true,
        }
    }
}

impl fmt::Display for PlaceProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Field(field) => write!(f, "{}", field),
            Self::Tuple(index) => write!(f, "[{index}]"),
            Self::Element(selector) | Self::Entry(selector) => write!(f, "[{selector}]"),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ProjectionPath(pub(super) Vec<PlaceProjection>);

impl ProjectionPath {
    pub(super) fn with_field(&self, field: impl Into<String>) -> Self {
        let mut projections = self.0.clone();
        projections.push(PlaceProjection::Field(field.into()));
        Self(projections)
    }

    pub(super) fn with_tuple(&self, index: usize) -> Self {
        let mut projections = self.0.clone();
        projections.push(PlaceProjection::Tuple(index));
        Self(projections)
    }

    pub(super) fn with_element(&self, selector: PlaceSelector) -> Self {
        let mut projections = self.0.clone();
        projections.push(PlaceProjection::Element(selector));
        Self(projections)
    }

    pub(super) fn with_entry(&self, selector: PlaceSelector) -> Self {
        let mut projections = self.0.clone();
        projections.push(PlaceProjection::Entry(selector));
        Self(projections)
    }

    pub(super) fn followed_by(&self, suffix: &Self) -> Self {
        let mut projections = self.0.clone();
        projections.extend(suffix.0.iter().cloned());
        Self(projections)
    }

    /// Two paths overlap unless some common depth names provably different
    /// places; an ancestor overlaps every descendant.
    pub(super) fn overlaps(&self, other: &Self) -> bool {
        !self
            .0
            .iter()
            .zip(other.0.iter())
            .any(|(left, right)| left.disjoint_from(right))
    }

    /// Whether `self` is `other` or a projection through it: an element or
    /// entry selection is only a descendant of the same selection or of a
    /// dynamic one, never of a different literal slot.
    pub(super) fn is_descendant_of_or_equal(&self, other: &Self) -> bool {
        self.0.len() >= other.0.len()
            && self
                .0
                .iter()
                .zip(other.0.iter())
                .all(|(left, right)| match (left, right) {
                    (PlaceProjection::Element(mine), PlaceProjection::Element(theirs))
                    | (PlaceProjection::Entry(mine), PlaceProjection::Entry(theirs)) => {
                        mine == theirs || matches!(theirs, PlaceSelector::Dynamic)
                    }
                    (
                        PlaceProjection::Tuple(position),
                        PlaceProjection::Element(theirs) | PlaceProjection::Entry(theirs),
                    ) => {
                        PlaceSelector::Int(*position as i128) == *theirs
                            || matches!(theirs, PlaceSelector::Dynamic)
                    }
                    (
                        PlaceProjection::Element(mine) | PlaceProjection::Entry(mine),
                        PlaceProjection::Tuple(position),
                    ) => *mine == PlaceSelector::Int(*position as i128),
                    _ => left == right,
                })
    }
}

impl fmt::Display for ProjectionPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, projection) in self.0.iter().enumerate() {
            if index > 0 && matches!(projection, PlaceProjection::Field(_)) {
                write!(f, ".")?;
            }
            write!(f, "{}", projection)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct PlacePath {
    pub(super) root: String,
    pub(super) projections: ProjectionPath,
}

impl PlacePath {
    pub(super) fn root(root: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            projections: ProjectionPath::default(),
        }
    }

    pub(super) fn with_field(&self, field: impl Into<String>) -> Self {
        Self {
            root: self.root.clone(),
            projections: self.projections.with_field(field),
        }
    }

    pub(super) fn with_tuple(&self, index: usize) -> Self {
        Self {
            root: self.root.clone(),
            projections: self.projections.with_tuple(index),
        }
    }

    pub(super) fn with_element(&self, selector: PlaceSelector) -> Self {
        Self {
            root: self.root.clone(),
            projections: self.projections.with_element(selector),
        }
    }

    pub(super) fn with_entry(&self, selector: PlaceSelector) -> Self {
        Self {
            root: self.root.clone(),
            projections: self.projections.with_entry(selector),
        }
    }

    pub(super) fn followed_by(&self, suffix: &ProjectionPath) -> Self {
        Self {
            root: self.root.clone(),
            projections: self.projections.followed_by(suffix),
        }
    }

    pub(super) fn overlaps(&self, other: &Self) -> bool {
        self.root == other.root && self.projections.overlaps(&other.projections)
    }

    pub(super) fn is_root(&self) -> bool {
        self.projections.0.is_empty()
    }
}

impl fmt::Display for PlacePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.root)?;
        if let Some(first) = self.projections.0.first() {
            if matches!(first, PlaceProjection::Field(_)) {
                write!(f, ".")?;
            }
            write!(f, "{}", self.projections)?;
        }
        Ok(())
    }
}

impl<'a> FunctionChecker<'a> {
    pub(super) fn borrow_call_place(&self, expr: &Expr) -> Option<PlacePath> {
        match &expr.kind {
            ExprKind::Name(name) => Some(PlacePath::root(name.clone())),
            ExprKind::Group(inner) => self.borrow_call_place(inner),
            ExprKind::Member { object, field } => {
                let parent = self.borrow_call_place(object)?;
                Some(parent.with_field(field.clone()))
            }
            ExprKind::Index { object, index } => {
                let ExprKind::Int(value) = index.kind else {
                    return None;
                };
                let index = usize::try_from(value).ok()?;
                self.borrow_call_place(object)
                    .map(|parent| parent.with_tuple(index))
            }
            _ => None,
        }
    }

    pub(super) fn place_path_type(
        &self,
        path: &PlacePath,
        locals: &HashMap<String, LocalBinding>,
        span: crate::diag::Span,
    ) -> Result<Option<Type>> {
        let Some(binding) = locals.get(&path.root) else {
            return Ok(None);
        };
        // A module-rooted path is a namespace or enum-variant path such as
        // `json.Value.Null`, not an owned place, so it never participates in
        // borrow or move tracking.
        if matches!(binding.ty, Type::Module(_)) {
            return Ok(None);
        }
        let mut ty = binding.ty.clone();
        for projection in &path.projections.0 {
            match projection {
                PlaceProjection::Field(field) => {
                    ty = self.resolve_member_type(&ty, field, span)?;
                }
                PlaceProjection::Tuple(index) => {
                    // A syntactic path spells every index as a position; on
                    // a list or dictionary it is the element or entry
                    // projection of `view_place`.
                    match &ty {
                        Type::Named(name, args) if name == "list" && args.len() == 1 => {
                            ty = args[0].clone();
                            continue;
                        }
                        Type::Named(name, args) if name == "dict" && args.len() == 2 => {
                            ty = args[1].clone();
                            continue;
                        }
                        _ => {}
                    }
                    let Type::Tuple(elements) = &ty else {
                        return Err(Diagnostic::coded_at(
                            "AU3004",
                            span,
                            format!("cannot project tuple position {index} from `{ty}`"),
                        ));
                    };
                    ty = elements.get(*index).cloned().ok_or_else(|| {
                        Diagnostic::coded_at(
                            "AU3004",
                            span,
                            format!("tuple has no position {index}"),
                        )
                    })?;
                }
                PlaceProjection::Element(_) => {
                    let element = match &ty {
                        Type::Named(name, args) if name == "list" && args.len() == 1 => {
                            Some(args[0].clone())
                        }
                        _ => None,
                    };
                    ty = element.ok_or_else(|| {
                        Diagnostic::coded_at(
                            "AU3004",
                            span,
                            format!("cannot project an element from `{ty}`"),
                        )
                    })?;
                }
                PlaceProjection::Entry(_) => {
                    let value = match &ty {
                        Type::Named(name, args) if name == "dict" && args.len() == 2 => {
                            Some(args[1].clone())
                        }
                        _ => None,
                    };
                    ty = value.ok_or_else(|| {
                        Diagnostic::coded_at(
                            "AU3004",
                            span,
                            format!("cannot project an entry from `{ty}`"),
                        )
                    })?;
                }
            }
        }
        Ok(Some(ty))
    }

    pub(super) fn render_member_target(&self, object: &Expr, field: &str) -> String {
        format!("{}.{}", self.render_place_expr(object), field)
    }

    pub(super) fn render_index_target(&self, object: &Expr) -> String {
        format!("{}[..]", self.render_place_expr(object))
    }

    pub(super) fn render_place_expr(&self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Name(name) => name.clone(),
            ExprKind::Group(inner) => self.render_place_expr(inner),
            ExprKind::Member { object, field } => {
                format!("{}.{}", self.render_place_expr(object), field)
            }
            ExprKind::Index { object, .. } => {
                format!("{}[..]", self.render_place_expr(object))
            }
            _ => "<place>".to_string(),
        }
    }

    pub(super) fn member_access_path(&self, expr: &Expr) -> Option<PlacePath> {
        match &expr.kind {
            ExprKind::Name(name) => Some(PlacePath::root(name.clone())),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. } => self.member_access_path(inner),
            ExprKind::Member { object, field } => {
                let parent = self.member_access_path(object)?;
                Some(parent.with_field(field.clone()))
            }
            // An indexed element or entry is a place of its collection: a
            // literal position is spelled as the syntactic position (one
            // slot with the element selector under the overlap rule), a
            // literal key as that entry, anything else as a dynamic
            // selection that overlaps every slot (ADR-0061, 2026-09-21
            // section).
            ExprKind::Index { object, index } => {
                let parent = self.member_access_path(object)?;
                Some(match &index.kind {
                    ExprKind::Int(value) => match usize::try_from(*value) {
                        Ok(position) => parent.with_tuple(position),
                        Err(_) => parent.with_element(PlaceSelector::Dynamic),
                    },
                    ExprKind::String(value) => parent.with_entry(PlaceSelector::Str(value.clone())),
                    ExprKind::Bool(value) => parent.with_entry(PlaceSelector::Bool(*value)),
                    _ => parent.with_element(PlaceSelector::Dynamic),
                })
            }
            _ => None,
        }
    }

    pub(super) fn member_target_path(&self, object: &Expr, field: &str) -> Option<PlacePath> {
        let parent = self.member_access_path(object)?;
        Some(parent.with_field(field.to_string()))
    }
}
