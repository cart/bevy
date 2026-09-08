use crate::parse_stream::{Ident, StringLit};

#[derive(Debug, Clone, PartialEq)]
pub struct BsnRoot(pub Bsn);

#[derive(Debug, Clone, PartialEq)]
pub struct Bsn {
    pub entries: Vec<BsnEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BsnEntry {
    Name(Ident),
    CachedScene(BsnScene),
    FromTemplatePatch(BsnType),
    RelatedSceneList {
        relationship_path: Path,
        scene_list: BsnSceneList,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BsnScene {
    Asset(StringLit),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BsnSceneList(pub BsnSceneListItems);

#[derive(Debug, Clone, PartialEq)]
pub struct BsnSceneListItems(pub Vec<Bsn>);

#[derive(Debug, Clone, PartialEq)]
pub struct BsnType {
    pub path: Path,
    pub variant: Option<Ident>,
    pub fields: BsnFields,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BsnFields {
    NamedFields(Vec<BsnNamedField>),
    UnnamedFields(Vec<BsnValue>),
    Unit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BsnNamedField {
    pub name: String,
    pub value: BsnValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BsnValue {
    Float(f64),
    Int(i128),
    Bool(bool),
    String(String),
    Type(BsnType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub leading_colon: bool,
    pub segments: Vec<PathSegment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathSegment {
    pub ident: Ident,
    // pub arguments: PathArguments
}

impl BsnType {
    pub fn new(path: impl Into<Path>, fields: impl Into<BsnFields>) -> Self {
        BsnType {
            path: path.into(),
            variant: None,
            fields: fields.into(),
        }
    }

    pub fn path(path: impl Into<Path>) -> Self {
        BsnType {
            path: path.into(),
            variant: None,
            fields: BsnFields::Unit,
        }
    }
}

impl<I: Into<String>> From<I> for PathSegment {
    fn from(value: I) -> Self {
        PathSegment {
            ident: Ident(value.into()),
        }
    }
}

impl Path {
    pub fn to_type_path(&self) -> String {
        // NOTE: skipping leading_colon here as type paths are normalized
        // PERF: could probably accurately reserve space here
        let mut value = String::default();
        for (index, segment) in self.segments.iter().enumerate() {
            value.push_str(&segment.ident.0);
            if index != self.segments.len() - 1 {
                value.push_str("::");
            }
        }
        value
    }
}

impl From<Vec<PathSegment>> for Path {
    fn from(segments: Vec<PathSegment>) -> Self {
        Self {
            leading_colon: false,
            segments,
        }
    }
}

impl<const LEN: usize, I: Into<String>> From<[I; LEN]> for Path {
    fn from(segments: [I; LEN]) -> Self {
        Self {
            leading_colon: false,
            segments: segments
                .into_iter()
                .map(|i| PathSegment::from(i))
                .collect::<Vec<_>>(),
        }
    }
}

impl BsnNamedField {
    pub fn new(name: impl Into<String>, value: BsnValue) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}
