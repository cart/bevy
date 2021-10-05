use bevy_asset::{AssetLoader, LoadContext, LoadedAsset};
use bevy_reflect::{TypeUuid, Uuid};
use bevy_utils::{tracing::error, BoxedFuture};
use naga::{valid::ModuleInfo, Module, ShaderStage};
use std::{borrow::Cow, collections::HashMap, marker::Copy};
use thiserror::Error;
use wgpu::{ShaderFlags, ShaderModuleDescriptor, ShaderSource};

use crate::render_asset::{PrepareAssetError, RenderAsset};

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct ShaderId(Uuid);

impl ShaderId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        ShaderId(Uuid::new_v4())
    }
}

#[derive(Error, Debug)]
pub enum ShaderReflectError {
    #[error(transparent)]
    WgslParse(#[from] naga::front::wgsl::ParseError),
    #[error(transparent)]
    GlslParse(#[from] naga::front::glsl::ParseError),
    #[error(transparent)]
    SpirVParse(#[from] naga::front::spv::Error),
    #[error(transparent)]
    Validation(#[from] naga::valid::ValidationError),
}

/// A shader, as defined by its [ShaderSource] and [ShaderStage]
/// This is an "unprocessed" shader. It can contain preprocessor directives.
#[derive(Debug, Clone, TypeUuid)]
#[uuid = "d95bc916-6c55-4de3-9622-37e7b6969fda"]
pub enum Shader {
    Wgsl(Cow<'static, str>),
    Glsl(Cow<'static, str>),
    SpirV(Cow<'static, [u8]>),
    // TODO: consider the following
    // PrecompiledSpirVMacros(HashMap<HashSet<String>, Vec<u32>>)
    // NagaModule(Module) ... Module impls Serialize/Deserialize
}

/// A processed [Shader]. This cannot contain preprocessor directions. It must be "ready to compile"
pub enum ProcessedShader {
    Wgsl(Cow<'static, str>),
    Glsl(Cow<'static, str>),
    SpirV(Cow<'static, [u8]>),
}

impl ProcessedShader {
    pub fn reflect(&self) -> Result<ShaderReflection, ShaderReflectError> {
        let module = match &self {
            // TODO: process macros here
            ProcessedShader::Wgsl(source) => naga::front::wgsl::parse_str(&source)?,
            ProcessedShader::Glsl(source) => {
                let mut entry_points = HashMap::default();
                entry_points.insert("vertex".to_string(), ShaderStage::Vertex);
                entry_points.insert("fragment".to_string(), ShaderStage::Fragment);
                naga::front::glsl::parse_str(
                    source,
                    &naga::front::glsl::Options {
                        entry_points,
                        defines: Default::default(),
                    },
                )?
            }
            ProcessedShader::SpirV(source) => naga::front::spv::parse_u8_slice(
                &source,
                &naga::front::spv::Options {
                    adjust_coordinate_space: false,
                    ..naga::front::spv::Options::default()
                },
            )?,
        };
        let module_info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::default(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)?;

        Ok(ShaderReflection {
            module,
            module_info,
        })
    }
}

pub struct ShaderReflection {
    pub module: Module,
    pub module_info: ModuleInfo,
}

impl ShaderReflection {
    pub fn get_spirv(&self) -> Result<Vec<u32>, naga::back::spv::Error> {
        naga::back::spv::write_vec(
            &self.module,
            &self.module_info,
            &naga::back::spv::Options {
                flags: naga::back::spv::WriterFlags::empty(),
                ..naga::back::spv::Options::default()
            },
        )
    }

    pub fn get_wgsl(&self) -> Result<String, naga::back::wgsl::Error> {
        naga::back::wgsl::write_string(&self.module, &self.module_info)
    }
}

impl Shader {
    pub fn from_wgsl(source: impl Into<Cow<'static, str>>) -> Shader {
        Shader::Wgsl(source.into())
    }

    pub fn from_glsl(source: impl Into<Cow<'static, str>>) -> Shader {
        Shader::Glsl(source.into())
    }

    pub fn from_spirv(source: impl Into<Cow<'static, [u8]>>) -> Shader {
        Shader::SpirV(source.into())
    }

    pub fn process(&self, shader_defs: &[String]) -> Option<ProcessedShader> {
        match self {
            Shader::Wgsl(source) => Some(ProcessedShader::Wgsl(source.clone())),
            Shader::Glsl(source) => Some(ProcessedShader::Glsl(source.clone())),
            Shader::SpirV(source) => {
                if shader_defs.is_empty() {
                    Some(ProcessedShader::SpirV(source.clone()))
                } else {
                    None
                }
            }
        }
    }
}

#[derive(Default)]
pub struct ShaderLoader;

impl AssetLoader for ShaderLoader {
    fn load<'a>(
        &'a self,
        bytes: &'a [u8],
        load_context: &'a mut LoadContext,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>> {
        Box::pin(async move {
            let ext = load_context.path().extension().unwrap().to_str().unwrap();

            let shader = match ext {
                "spv" => Shader::from_spirv(Vec::from(bytes)),
                "wgsl" => Shader::from_wgsl(String::from_utf8(Vec::from(bytes))?),
                "vert" => Shader::from_glsl(String::from_utf8(Vec::from(bytes))?),
                "frag" => Shader::from_glsl(String::from_utf8(Vec::from(bytes))?),
                _ => panic!("unhandled extension: {}", ext),
            };

            load_context.set_default_asset(LoadedAsset::new(shader));
            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        &["spv", "wgsl", "vert", "frag"]
    }
}

impl<'a> From<&'a ProcessedShader> for ShaderModuleDescriptor<'a> {
    fn from(shader: &'a ProcessedShader) -> Self {
        ShaderModuleDescriptor {
            flags: ShaderFlags::default(),
            label: None,
            source: match shader {
                ProcessedShader::Wgsl(source) => ShaderSource::Wgsl(source.clone()),
                ProcessedShader::Glsl(_source) => {
                    let reflection = shader.reflect().unwrap();
                    // TODO: it probably makes more sense to convert this to spirv, but as of writing
                    // this comment, naga's spirv conversion is broken
                    let wgsl = reflection.get_wgsl().unwrap();
                    ShaderSource::Wgsl(wgsl.into())
                }
                ProcessedShader::SpirV(_) => {
                    // TODO: we can probably just transmute the u8 array to u32?
                    let reflection = shader.reflect().unwrap();
                    let spirv = reflection.get_spirv().unwrap();
                    ShaderSource::SpirV(Cow::Owned(spirv))
                }
            },
        }
    }
}

impl RenderAsset for Shader {
    type ExtractedAsset = Shader;
    type PreparedAsset = Shader;
    type Param = ();

    fn extract_asset(&self) -> Self::ExtractedAsset {
        self.clone()
    }

    fn prepare_asset(
        extracted_asset: Self::ExtractedAsset,
        _param: &mut bevy_ecs::system::SystemParamItem<Self::Param>,
    ) -> Result<Self::PreparedAsset, PrepareAssetError<Self::ExtractedAsset>> {
        Ok(extracted_asset)
    }
}
