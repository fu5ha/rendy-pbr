//! Scene graph system and types

use crate::transform::{GlobalTransform, HierarchyEvent, Parent, ParentHierarchy, Transform};

use specs::prelude::{
    ComponentEvent, Entities, Entity, Join, ReadExpect, ReadStorage, ReaderId, Resources, System,
    WriteStorage,
};

use hibitset::BitSet;

pub struct TransformSystem {
    local_modified: BitSet,
    global_modified: BitSet,

    locals_events_id: Option<ReaderId<ComponentEvent>>,

    parent_events_id: Option<ReaderId<HierarchyEvent>>,

    scratch: Vec<Entity>,
}

impl TransformSystem {
    /// Creates a new transform processor.
    pub fn new() -> TransformSystem {
        TransformSystem {
            locals_events_id: None,
            parent_events_id: None,
            local_modified: BitSet::default(),
            global_modified: BitSet::default(),
            scratch: Vec::new(),
        }
    }
}

impl<'a> System<'a> for TransformSystem {
    type SystemData = (
        Entities<'a>,
        ReadExpect<'a, ParentHierarchy>,
        ReadStorage<'a, Transform>,
        ReadStorage<'a, Parent>,
        WriteStorage<'a, GlobalTransform>,
    );
    fn run(&mut self, (entities, hierarchy, locals, parents, mut globals): Self::SystemData) {
        self.local_modified.clear();
        self.global_modified.clear();

        self.scratch.clear();
        self.scratch
            .extend((&*entities, &locals, !&globals).join().map(|d| d.0));
        for entity in &self.scratch {
            globals
                .insert(*entity, GlobalTransform::default())
                .expect("unreachable");
            self.local_modified.add(entity.id());
        }

        locals
            .channel()
            .read(
                self.locals_events_id.as_mut().expect(
                    "`TransformSystem::setup` was not called before `TransformSystem::run`",
                ),
            )
            .for_each(|event| match event {
                ComponentEvent::Inserted(id) | ComponentEvent::Modified(id) => {
                    self.local_modified.add(*id);
                }
                ComponentEvent::Removed(_id) => {}
            });

        for event in hierarchy.changed().read(
            self.parent_events_id
                .as_mut()
                .expect("`TransformSystem::setup` was not called before `TransformSystem::run`"),
        ) {
            match *event {
                HierarchyEvent::Removed(entity) => {
                    // Sometimes the user may have already deleted the entity.
                    // This is fine, so we'll ignore any errors this may give
                    // since it can only fail due to the entity already being dead.
                    let _ = entities.delete(entity);
                }
                HierarchyEvent::Modified(entity) => {
                    self.local_modified.add(entity.id());
                }
            }
        }

        // Compute transforms without parents.
        for (entity, _, local, global, _) in (
            &*entities,
            &self.local_modified,
            &locals,
            &mut globals,
            !&parents,
        )
            .join()
        {
            self.global_modified.add(entity.id());
            global.0 = local.0.to_homogeneous();
            // log::debug!("Baked local transform: {} to global: {}", local.0, global.0);
            debug_assert!(
                global.is_finite(),
                format!("Entity {:?} had a non-finite `Transform`", entity)
            );
        }

        // Compute transforms with parents.
        for entity in hierarchy.all() {
            let self_dirty = self.local_modified.contains(entity.id());
            if let (Some(parent), Some(local)) = (parents.get(*entity), locals.get(*entity)) {
                let parent_dirty = self.global_modified.contains(parent.entity.id());
                if parent_dirty || self_dirty {
                    let combined_transform = if let Some(parent_global) = globals.get(parent.entity)
                    {
                        (parent_global.0 * local.0.to_homogeneous())
                    } else {
                        local.0.to_homogeneous()
                    };

                    if let Some(global) = globals.get_mut(*entity) {
                        self.global_modified.add(entity.id());
                        global.0 = combined_transform;
                        // log::debug!(
                        //     "Baked local transform (WTH PARENT): {} to global: {}",
                        //     local.0,
                        //     global.0
                        // );
                    }
                }
            }
        }
    }

    fn setup(&mut self, res: &mut Resources) {
        use specs::prelude::SystemData;
        Self::SystemData::setup(res);
        let mut hierarchy = res.fetch_mut::<ParentHierarchy>();
        let mut locals = WriteStorage::<Transform>::fetch(res);
        self.parent_events_id = Some(hierarchy.track());
        self.locals_events_id = Some(locals.register_reader());
    }
}
