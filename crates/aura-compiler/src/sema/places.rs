//! Canonical place paths, projections, overlap, and expression paths.

use super::{
    fmt, Diagnostic, Expr, ExprKind, FunctionChecker, HashMap, LocalBinding, Result, Type,
};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum PlaceProjection {
    Field(String),
    Tuple(usize),
}

impl fmt::Display for PlaceProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Field(field) => write!(f, "{}", field),
            Self::Tuple(index) => write!(f, "[{index}]"),
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

    pub(super) fn followed_by(&self, suffix: &Self) -> Self {
        let mut projections = self.0.clone();
        projections.extend(suffix.0.iter().cloned());
        Self(projections)
    }

    pub(super) fn overlaps(&self, other: &Self) -> bool {
        self.0.starts_with(&other.0) || other.0.starts_with(&self.0)
    }

    pub(super) fn is_descendant_of_or_equal(&self, other: &Self) -> bool {
        self.0.starts_with(&other.0)
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
            _ => None,
        }
    }

    pub(super) fn member_target_path(&self, object: &Expr, field: &str) -> Option<PlacePath> {
        let parent = self.member_access_path(object)?;
        Some(parent.with_field(field.to_string()))
    }
}
