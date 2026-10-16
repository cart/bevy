mod enum_;
mod primitive;
mod struct_;

pub use enum_::*;
pub use primitive::*;
pub use struct_::*;

#[derive(Debug)]
pub enum Kind {
    Struct(Struct),
    Primitive(Primitive),
    Enum(Enum),
}
