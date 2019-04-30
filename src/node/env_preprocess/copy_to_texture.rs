use rendy::{
    command::{
        CommandBuffer, CommandPool, ExecutableState, Family, FamilyId, Fence, MultiShot,
        PendingState, Queue, SimultaneousUse, Submission, Submit, Supports, Transfer,
    },
    factory::{Factory, ImageState},
    frame::Frames,
    graph::{
        gfx_acquire_barriers, gfx_release_barriers, BufferAccess, BufferId, DynNode, GraphContext,
        ImageAccess, ImageId, NodeBuffer, NodeBuilder, NodeId, NodeImage,
    },
    texture::Texture,
};

use rendy::hal;

#[derive(Debug)]
pub struct CopyToTexture<B: hal::Backend> {
    pool: CommandPool<B, hal::QueueType>,
    submit: Submit<B, SimultaneousUse>,
    buffer:
        CommandBuffer<B, hal::QueueType, PendingState<ExecutableState<MultiShot<SimultaneousUse>>>>,
}

impl<B: hal::Backend> CopyToTexture<B> {
    pub fn builder(input: ImageId, output_tex_name: &str) -> CopyToTextureBuilder {
        CopyToTextureBuilder {
            input,
            output_tex_name: String::from(output_tex_name),
            dependencies: vec![],
        }
    }
}

#[derive(Debug)]
pub struct CopyToTextureBuilder {
    input: ImageId,
    output_tex_name: String,
    dependencies: Vec<NodeId>,
}

impl CopyToTextureBuilder {
    /// Add dependency.
    /// Node will be placed after its dependencies.
    pub fn add_dependency(&mut self, dependency: NodeId) -> &mut Self {
        self.dependencies.push(dependency);
        self
    }

    /// Add dependency.
    /// Node will be placed after its dependencies.
    pub fn with_dependency(mut self, dependency: NodeId) -> Self {
        self.add_dependency(dependency);
        self
    }
}

pub trait CopyToTextureResource<B: hal::Backend> {
    fn get_texture(&self, name: &str) -> &Texture<B>;
    fn texture_end_state(&self, name: &str) -> ImageState;
}

impl<B, TR> NodeBuilder<B, TR> for CopyToTextureBuilder
where
    B: hal::Backend,
    TR: CopyToTextureResource<B>,
{
    fn family(&self, _factory: &mut Factory<B>, families: &[Family<B>]) -> Option<FamilyId> {
        families
            .iter()
            .find(|family| Supports::<Transfer>::supports(&family.capability()).is_some())
            .map(|family| family.id())
    }

    fn buffers(&self) -> Vec<(BufferId, BufferAccess)> {
        Vec::new()
    }

    fn images(&self) -> Vec<(ImageId, ImageAccess)> {
        vec![(
            self.input,
            ImageAccess {
                access: hal::image::Access::TRANSFER_READ,
                layout: hal::image::Layout::TransferSrcOptimal,
                usage: hal::image::Usage::TRANSFER_SRC,
                stages: hal::pso::PipelineStage::TRANSFER,
            },
        )]
    }

    fn dependencies(&self) -> Vec<NodeId> {
        self.dependencies.clone()
    }

    fn build<'a>(
        self: Box<Self>,
        ctx: &GraphContext<B>,
        factory: &mut Factory<B>,
        family: &mut Family<B>,
        _queue: usize,
        aux: &TR,
        buffers: Vec<NodeBuffer>,
        images: Vec<NodeImage>,
    ) -> Result<Box<dyn DynNode<B, TR>>, failure::Error> {
        assert_eq!(buffers.len(), 0);
        assert_eq!(images.len(), 1);

        let mut pool = factory.create_command_pool(family)?;

        let buf_initial = pool.allocate_buffers(1).pop().unwrap();
        let mut buf_recording = buf_initial.begin(MultiShot(SimultaneousUse), ());
        let mut encoder = buf_recording.encoder();
        let target_tex = aux.get_texture(&self.output_tex_name);

        {
            let (stages, barriers) = gfx_acquire_barriers(ctx, None, images.iter());
            log::trace!("Acquire {:?} : {:#?}", stages, barriers);
            if !barriers.is_empty() {
                encoder.pipeline_barrier(stages, hal::memory::Dependencies::empty(), barriers);
            }
        }

        let image = ctx.get_image(images[0].id).unwrap();
        encoder.copy_image(
            image.raw(),
            images[0].layout,
            target_tex.image().raw(),
            hal::image::Layout::TransferDstOptimal,
            Some(hal::command::ImageCopy {
                src_subresource: hal::image::SubresourceLayers {
                    aspects: hal::format::Aspects::COLOR,
                    level: 0,
                    layers: 0..1,
                },
                src_offset: hal::image::Offset::ZERO,
                dst_subresource: hal::image::SubresourceLayers {
                    aspects: hal::format::Aspects::COLOR,
                    level: 0,
                    layers: 0..1,
                },
                dst_offset: hal::image::Offset::ZERO,
                extent: hal::image::Extent {
                    width: image.kind().extent().width,
                    height: image.kind().extent().height,
                    depth: 1,
                },
            }),
        );

        {
            let (mut stages, mut barriers) = gfx_release_barriers(ctx, None, images.iter());
            let end_state = aux.texture_end_state(&self.output_tex_name);
            stages.start |= hal::pso::PipelineStage::TRANSFER;
            stages.end |= end_state.stage;
            barriers.push(hal::memory::Barrier::Image {
                states: (
                    hal::image::Access::TRANSFER_WRITE,
                    hal::image::Layout::TransferDstOptimal,
                )..(end_state.access, end_state.layout),
                families: None,
                target: target_tex.image().raw(),
                range: hal::image::SubresourceRange {
                    aspects: hal::format::Aspects::COLOR,
                    levels: 0..1,
                    layers: 0..1,
                },
            });

            log::trace!("Release {:?} : {:#?}", stages, barriers);
            encoder.pipeline_barrier(stages, hal::memory::Dependencies::empty(), barriers);
        }

        let (submit, buffer) = buf_recording.finish().submit();

        Ok(Box::new(CopyToTexture {
            pool,
            submit,
            buffer,
        }))
    }
}

impl<B, TR> DynNode<B, TR> for CopyToTexture<B>
where
    B: hal::Backend,
    TR: CopyToTextureResource<B>,
{
    unsafe fn run<'a>(
        &mut self,
        _ctx: &GraphContext<B>,
        _factory: &Factory<B>,
        queue: &mut Queue<B>,
        _aux: &TR,
        _frames: &Frames<B>,
        waits: &[(&'a B::Semaphore, hal::pso::PipelineStage)],
        signals: &[&'a B::Semaphore],
        fence: Option<&mut Fence<B>>,
    ) {
        queue.submit(
            Some(
                Submission::new()
                    .submits(Some(&self.submit))
                    .wait(waits.iter().cloned())
                    .signal(signals.iter()),
            ),
            fence,
        );
    }

    unsafe fn dispose(mut self: Box<Self>, factory: &mut Factory<B>, _aux: &TR) {
        drop(self.submit);
        self.pool.free_buffers(Some(self.buffer.mark_complete()));
        factory.destroy_command_pool(self.pool);
    }
}
