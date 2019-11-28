use rendy::{
    command::{QueueId, RenderPassEncoder},
    factory::Factory,
    graph::{render::*, GraphContext, NodeBuffer, NodeImage},
    resource::{DescriptorSetLayout, Handle},
    shader::{PathBufShaderInfo, ShaderKind, SourceLanguage},
};

use rendy::hal;

use crate::node::env_preprocess::Aux;

lazy_static::lazy_static! {
    static ref VERTEX: PathBufShaderInfo = PathBufShaderInfo::new(
        std::path::PathBuf::from(crate::application_root_dir()).join("assets/shaders/fullscreen_triangle.vert"),
        ShaderKind::Vertex,
        SourceLanguage::GLSL,
        "main",
    );

    static ref FRAGMENT: PathBufShaderInfo = PathBufShaderInfo::new(
        std::path::PathBuf::from(crate::application_root_dir()).join("assets/shaders/integrate_spec_brdf.frag"),
        ShaderKind::Fragment,
        SourceLanguage::GLSL,
        "main",
    );

    static ref SHADERS: rendy::shader::ShaderSetBuilder = rendy::shader::ShaderSetBuilder::default()
        .with_vertex(&*VERTEX).unwrap()
        .with_fragment(&*FRAGMENT).unwrap();
}

#[derive(Debug, Default)]
pub struct PipelineDesc;

pub struct Pipeline;

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Integrate Spec BRDF Pipeline")
    }
}

impl<B> SimpleGraphicsPipelineDesc<B, Aux<B>> for PipelineDesc
where
    B: hal::Backend,
{
    type Pipeline = Pipeline;

    fn colors(&self) -> Vec<hal::pso::ColorBlendDesc> {
        vec![hal::pso::ColorBlendDesc {
            mask: hal::pso::ColorMask::RED | hal::pso::ColorMask::GREEN,
            blend: None,
        }]
    }

    fn depth_stencil(&self) -> Option<hal::pso::DepthStencilDesc> {
        None
    }

    fn load_shader_set(
        &self,
        factory: &mut Factory<B>,
        _aux: &Aux<B>,
    ) -> rendy::shader::ShaderSet<B> {
        SHADERS.build(factory, Default::default()).unwrap()
    }

    fn layout(&self) -> Layout {
        Layout {
            sets: Vec::new(),
            push_constants: Vec::new(),
        }
    }

    fn build<'a>(
        self,
        _ctx: &GraphContext<B>,
        _factory: &mut Factory<B>,
        _queue: QueueId,
        _aux: &Aux<B>,
        _buffers: Vec<NodeBuffer>,
        _images: Vec<NodeImage>,
        _set_layouts: &[Handle<DescriptorSetLayout<B>>],
    ) -> Result<Pipeline, hal::pso::CreationError> {
        Ok(Pipeline)
    }
}

impl<B> SimpleGraphicsPipeline<B, Aux<B>> for Pipeline
where
    B: hal::Backend,
{
    type Desc = PipelineDesc;

    fn prepare(
        &mut self,
        _factory: &Factory<B>,
        _queue: QueueId,
        _set_layouts: &[Handle<DescriptorSetLayout<B>>],
        _index: usize,
        _aux: &Aux<B>,
    ) -> PrepareResult {
        PrepareResult::DrawReuse
    }

    fn draw(
        &mut self,
        _layout: &B::PipelineLayout,
        mut encoder: RenderPassEncoder<'_, B>,
        _index: usize,
        _aux: &Aux<B>,
    ) {
        unsafe {
            encoder.draw(0..3, 0..1);
        }
    }

    fn dispose(self, _factory: &mut Factory<B>, _aux: &Aux<B>) {}
}
