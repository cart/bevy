use alloc::boxed::Box;
use core::any::{Any, TypeId};

use bevy_reflect::{CreateTypeData, Reflect};
use derive_more::{Deref, DerefMut};

use crate::{
    component::Component,
    error::BevyError,
    prelude::{FromTemplate, Template},
    template::{ErasedTemplate, SceneEffect, TemplateContext},
};

#[derive(Clone, Deref, DerefMut)]
pub struct ReflectFromTemplate(pub ReflectFromTemplateData);

#[derive(Clone, Deref, DerefMut)]
pub struct ReflectTemplate(pub ReflectTemplateData);

#[derive(Clone)]
pub struct ReflectFromTemplateData {
    pub template_type_id: TypeId,
}

#[derive(Clone)]
pub struct ReflectTemplateData {
    pub build_template: BuildTemplateFn,
    pub into_erased: IntoErasedFn,
    pub reflect_erased: ReflectErasedFn,
}

pub type IntoErasedFn = fn(Box<dyn Reflect>) -> Box<dyn ErasedTemplate>;
pub type ReflectErasedFn = fn(&mut dyn ErasedTemplate) -> &mut dyn Reflect;

pub type BuildTemplateFn =
    fn(&dyn Reflect, &mut TemplateContext) -> Result<Box<dyn Reflect>, BevyError>;

impl<T: Component + FromTemplate<Template: Template<Output: Reflect> + Send + Sync + 'static>>
    CreateTypeData<T> for ReflectFromTemplate
{
    fn create_type_data(_input: ()) -> Self {
        ReflectFromTemplate(ReflectFromTemplateData {
            template_type_id: TypeId::of::<T::Template>(),
        })
    }
}

impl<T: Template<Output: Reflect + SceneEffect> + Reflect + Send + Sync + 'static> CreateTypeData<T>
    for ReflectTemplate
{
    fn create_type_data(_input: ()) -> Self {
        ReflectTemplate(ReflectTemplateData {
            build_template: |this, context| {
                let Some(this) = this.downcast_ref::<T>() else {
                    return Err("Unexpected `build_template` receiver type".into());
                };
                Ok(Box::new(<T as Template>::build_template(this, context)?))
            },
            into_erased: |reflect| {
                let t = reflect.take::<T>().unwrap();
                Box::new(t)
            },
            reflect_erased: |erased_template| {
                let value = (erased_template as &mut dyn Any)
                    .downcast_mut::<T>()
                    .unwrap();
                value as &mut dyn Reflect
            },
        })
    }
}
