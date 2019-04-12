use rendy::{
    command::{
        CommandBuffer, CommandPool, ExecutableState, Family, FamilyId, Fence, MultiShot,
        PendingState, Queue, SimultaneousUse, Submission, Submit, Supports, Transfer,
    },
    factory::{Factory, ImageState},
    frame::Frames,
    graph::{
        gfx_acquire_barriers, gfx_release_barriers, BufferAccess, BufferId, DynNode, ImageAccess,
        ImageId, NodeBuffer, NodeBuilder, NodeId, NodeImage,
    },
    texture::Texture,
};

use rendy::hal;

#[derive(Debug)]
pub struct FacesToCubemap<B: hal::Backend> {
    pool: CommandPool<B, hal::QueueType>,
    submit: Submit<B, SimultaneousUse>,
    buffer:
        CommandBuffer<B, hal::QueueType, PendingState<ExecutableState<MultiShot<SimultaneousUse>>>>,
}

impl<B: hal::Backend> FacesToCubemap<B> {
    pub fn builder(faces: Vec<ImageId>) -> FacesToCubemapBuilder {
        assert_eq!(faces.len(), 6);
        FacesToCubemapBuilder {
            faces,
            dependencies: vec![],
        }
    }
}

#[derive(Debug)]
pub struct FacesToCubemapBuilder {
    faces: Vec<ImageId>,
    dependencies: Vec<NodeId>,
}

impl FacesToCubemapBuilder {
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

pub trait FacesToCubemapResource<B: hal::Backend> {
    fn get_cubemap(&self) -> &Texture<B>;
    fn cubemap_end_state(&self) -> ImageState;
}

impl<B, FR> NodeBuilder<B, FR> for FacesToCubemapBuilder
where
    B: hal::Backend,
    FR: FacesToCubemapResource<B>,
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
        self.faces
            .iter()
            .map(|&image| {
                (
                    image,
                    ImageAccess {
                        access: hal::image::Access::TRANSFER_READ,
                        layout: hal::image::Layout::TransferSrcOptimal,
                        usage: hal::image::Usage::TRANSFER_SRC,
                        stages: hal::pso::PipelineStage::TRANSFER,
                    },
                )
            })
            .collect::<_>()
    }

    fn dependencies(&self) -> Vec<NodeId> {
        self.dependencies.clone()
    }

    fn build<'a>(
        self: Box<Self>,
        factory: &mut Factory<B>,
        family: &mut Family<B>,
        _queue: usize,
        aux: &mut FR,
        buffers: Vec<NodeBuffer<'a, B>>,
        images: Vec<NodeImage<'a, B>>,
    ) -> Result<Box<dyn DynNode<B, FR>>, failure::Error> {
        assert_eq!(buffers.len(), 0);
        assert_eq!(images.len(), 6);

        let mut pool = factory.create_command_pool(family)?;

        let buf_initial = pool.allocate_buffers(1).pop().unwrap();
        let mut buf_recording = buf_initial.begin(MultiShot(SimultaneousUse), ());
        let mut encoder = buf_recording.encoder();
        let target_cubemap = aux.get_cubemap();

        {
            let (stages, barriers) = gfx_acquire_barriers(None, images.iter());
            log::info!("Acquire {:?} : {:#?}", stages, barriers);
            if !barriers.is_empty() {
                encoder.pipeline_barrier(stages, hal::memory::Dependencies::empty(), barriers);
            }
        }
        for (i, face) in images.iter().enumerate() {
            let i = i as u16;
            encoder.copy_image(
                face.image.raw(),
                face.layout,
                target_cubemap.image.raw(),
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
                        layers: i..i + 1,
                    },
                    dst_offset: hal::image::Offset::ZERO,
                    extent: hal::image::Extent {
                        width: face.image.kind().extent().width,
                        height: face.image.kind().extent().height,
                        depth: 1,
                    },
                }),
            );
        }
        {
            let (mut stages, mut barriers) = gfx_release_barriers(None, images.iter());
            let end_state = aux.cubemap_end_state();
            stages.start |= hal::pso::PipelineStage::TRANSFER;
            stages.end |= end_state.stage;
            barriers.push(hal::memory::Barrier::Image {
                states: (
                    hal::image::Access::TRANSFER_WRITE,
                    hal::image::Layout::TransferDstOptimal,
                )..(end_state.access, end_state.layout),
                families: None,
                target: target_cubemap.image.raw(),
                range: hal::image::SubresourceRange {
                    aspects: hal::format::Aspects::COLOR,
                    levels: 0..1,
                    layers: 0..6,
                },
            });

            log::info!("Release {:?} : {:#?}", stages, barriers);
            encoder.pipeline_barrier(stages, hal::memory::Dependencies::empty(), barriers);
        }

        let (submit, buffer) = buf_recording.finish().submit();

        Ok(Box::new(FacesToCubemap {
            pool,
            submit,
            buffer,
        }))
    }
}

impl<B, FR> DynNode<B, FR> for FacesToCubemap<B>
where
    B: hal::Backend,
    FR: FacesToCubemapResource<B>,
{
    unsafe fn run<'a>(
        &mut self,
        _factory: &Factory<B>,
        queue: &mut Queue<B>,
        _aux: &FR,
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

    unsafe fn dispose(mut self: Box<Self>, factory: &mut Factory<B>, _aux: &mut FR) {
        drop(self.submit);
        self.pool.free_buffers(Some(self.buffer.mark_complete()));
        factory.destroy_command_pool(self.pool);
    }
}
