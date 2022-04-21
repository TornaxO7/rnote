use super::StoreTask;
use super::{Stroke, StrokeKey, StrokeStore};
use crate::engine::visual_debug;
use crate::strokes::strokebehaviour::GeneratedStrokeImages;
use crate::strokes::StrokeBehaviour;
use crate::utils::{GdkRGBAHelpers, GrapheneRectHelpers};
use crate::{render, DrawBehaviour};

use anyhow::Context;
use gtk4::{gdk, graphene, gsk, Snapshot};
use p2d::bounding_volume::{BoundingVolume, AABB};
use rnote_compose::color;
use rnote_compose::shapes::ShapeBehaviour;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename = "render_component")]
pub struct RenderComponent {
    #[serde(rename = "render")]
    pub render: bool,
    #[serde(skip)]
    pub regenerate_flag: bool,
    #[serde(skip)]
    pub images: Vec<render::Image>,
    #[serde(skip)]
    pub rendernodes: Vec<gsk::RenderNode>,
}

impl Default for RenderComponent {
    fn default() -> Self {
        Self {
            render: true,
            regenerate_flag: true,
            images: vec![],
            rendernodes: vec![],
        }
    }
}

impl StrokeStore {
    /// Returns false if rendering is not supported
    pub fn can_render(&self, key: StrokeKey) -> bool {
        self.render_components.get(key).is_some()
    }

    /// Wether rendering is enabled. Returns None if rendering is not supported
    pub fn does_render(&self, key: StrokeKey) -> Option<bool> {
        if let Some(render_comp) = self.render_components.get(key) {
            Some(render_comp.render)
        } else {
            log::debug!(
                "get render_comp failed in does_render() of stroke for key {:?}, invalid key used or stroke does not support rendering",
                key
            );
            None
        }
    }

    pub fn set_render(&mut self, key: StrokeKey, render: bool) {
        if let Some(render_component) = self.render_components.get_mut(key) {
            render_component.render = render;
        } else {
            log::debug!(
                "get render_comp failed in set_render() of stroke for key {:?}, invalid key used or stroke does not support rendering",
                key
            );
        }
    }

    pub fn regenerate_flag(&self, key: StrokeKey) -> Option<bool> {
        if let Some(render_comp) = self.render_components.get(key) {
            Some(render_comp.regenerate_flag)
        } else {
            log::debug!(
                "get render_comp failed in regenerate_flag() of stroke for key {:?}, invalid key used or stroke does not support rendering",
                key
            );
            None
        }
    }

    pub fn set_regenerate_flag(&mut self, key: StrokeKey, regenerate_flag: bool) {
        if let Some(render_comp) = self.render_components.get_mut(key) {
            render_comp.regenerate_flag = regenerate_flag;
        } else {
            log::debug!(
                "get render_comp failed in set_regenerate_flag() of stroke for key {:?}, invalid key used or stroke does not support rendering",
                key
            );
        }
    }

    pub fn reset_regenerate_flag_all_strokes(&mut self) {
        self.render_components
            .iter_mut()
            .for_each(|(_key, render_comp)| {
                render_comp.regenerate_flag = true;
            });
    }

    pub fn regenerate_rendering_for_stroke(
        &mut self,
        key: StrokeKey,
        viewport: AABB,
        image_scale: f64,
    ) -> anyhow::Result<()> {
        if let (Some(stroke), Some(render_comp)) =
            (self.strokes.get(key), self.render_components.get_mut(key))
        {
            let images = stroke
                .gen_images(viewport, image_scale)
                .context("gen_images() failed  in regenerate_rendering_for_stroke()")?;

            match images {
                GeneratedStrokeImages::Partial(images) => {
                    let rendernodes = render::Image::images_to_rendernodes(&images).context(
                        " image_to_rendernode() failed in regenerate_rendering_for_stroke()",
                    )?;

                    render_comp.rendernodes = rendernodes;
                    render_comp.images = images;
                    render_comp.regenerate_flag = true;
                }
                GeneratedStrokeImages::Full(images) => {
                    let rendernodes = render::Image::images_to_rendernodes(&images).context(
                        " image_to_rendernode() failed in regenerate_rendering_for_stroke()",
                    )?;

                    render_comp.rendernodes = rendernodes;
                    render_comp.images = images;
                    render_comp.regenerate_flag = false;
                }
            }
        }
        Ok(())
    }

    pub fn regenerate_rendering_for_strokes(
        &mut self,
        keys: &[StrokeKey],
        viewport: AABB,
        image_scale: f64,
    ) -> anyhow::Result<()> {
        for key in keys.iter() {
            self.regenerate_rendering_for_stroke(*key, viewport, image_scale)?;
        }
        Ok(())
    }

    pub fn regenerate_rendering_for_stroke_threaded(
        &mut self,
        key: StrokeKey,
        viewport: AABB,
        image_scale: f64,
    ) {
        let tasks_tx = self.tasks_tx.clone();

        if let (Some(render_comp), Some(stroke)) =
            (self.render_components.get_mut(key), self.strokes.get(key))
        {
            let stroke = stroke.clone();

            render_comp.regenerate_flag = !viewport.contains(&stroke.bounds());

            // Spawn a new thread for image rendering
            self.threadpool.spawn(move || {
                match stroke.gen_images(viewport, image_scale) {
                    Ok(images) => {
                        tasks_tx.unbounded_send(StoreTask::UpdateStrokeWithImages {
                            key,
                            images,
                        }).unwrap_or_else(|e| {
                            log::error!("tasks_tx.send() UpdateStrokeWithImages failed in regenerate_rendering_for_stroke_threaded() for stroke with key {:?}, with Err, {}",key, e);
                        });
                    }
                    Err(e) => {
                        log::debug!("stroke.gen_image() failed in regenerate_rendering_for_stroke_threaded() for stroke with key {:?}, with Err {}", key, e);
                    }
                }
            });
        }
    }

    pub fn regenerate_rendering_for_strokes_threaded(
        &mut self,
        keys: &[StrokeKey],
        viewport: AABB,
        image_scale: f64,
    ) {
        keys.iter().for_each(|&key| {
            self.regenerate_rendering_for_stroke_threaded(key, viewport, image_scale);
        })
    }

    pub fn regenerate_rendering_in_viewport(
        &mut self,
        force_regenerate: bool,
        viewport: AABB,
        image_scale: f64,
    ) -> anyhow::Result<()> {
        let keys = self.render_components.keys().collect::<Vec<StrokeKey>>();

        for key in keys {
            if let (Some(stroke), Some(render_comp)) =
                (self.strokes.get(key), self.render_components.get_mut(key))
            {
                // skip and empty image buffer if stroke is not in expanded viewport
                if !viewport.intersects(&stroke.bounds()) {
                    render_comp.rendernodes = vec![];
                    render_comp.images = vec![];
                    render_comp.regenerate_flag = true;

                    continue;
                }
                // or does not need regeneration
                if !force_regenerate && !render_comp.regenerate_flag {
                    continue;
                }

                self.regenerate_rendering_for_stroke(key, viewport, image_scale)?;
            }
        }
        Ok(())
    }

    pub fn regenerate_rendering_in_viewport_threaded(
        &mut self,
        force_regenerate: bool,
        viewport: AABB,
        image_scale: f64,
    ) {
        let keys = self.render_components.keys().collect::<Vec<StrokeKey>>();

        keys.into_iter().for_each(|key| {
            if let (Some(stroke), Some(render_comp)) =
                (self.strokes.get(key), self.render_components.get_mut(key))
            {
                // skip and empty image buffer if stroke is not in expanded viewport
                if !viewport.intersects(&stroke.bounds()) {
                    render_comp.rendernodes = vec![];
                    render_comp.images = vec![];
                    render_comp.regenerate_flag = true;

                    return;
                }
                // or does not need regeneration
                if !force_regenerate && !render_comp.regenerate_flag {
                    return;
                }

                let tasks_tx = self.tasks_tx.clone();
                let stroke = stroke.clone();

                // Only set false when viewport contains stroke bounds
                render_comp.regenerate_flag = !viewport.contains(&stroke.bounds());

                // Spawn a new thread for image rendering
                self.threadpool.spawn(move || {
                    match stroke.gen_images(viewport, image_scale) {
                        Ok(images) => {
                            tasks_tx.unbounded_send(StoreTask::UpdateStrokeWithImages {
                                key,
                                images,
                            }).unwrap_or_else(|e| {
                                log::error!("tasks_tx.send() UpdateStrokeWithImages failed in regenerate_rendering_in_viewport_threaded() for stroke with key {:?}, with Err, {}",key, e);
                            });
                        }
                        Err(e) => {
                            log::debug!("stroke.gen_image() failed in regenerate_rendering_in_viewport_threaded() for stroke with key {:?}, with Err {}", key, e);
                        }
                    }
                });
            }
        })
    }

    /// generates images and appends them to the render component for the last segments of brushstrokes. For other strokes all rendering is regenerated
    pub fn append_rendering_last_segments(
        &mut self,
        key: StrokeKey,
        n_segments: usize,
        viewport: AABB,
        image_scale: f64,
    ) -> anyhow::Result<()> {
        if let (Some(stroke), Some(render_comp)) =
            (self.strokes.get(key), self.render_components.get_mut(key))
        {
            match stroke {
                Stroke::BrushStroke(brushstroke) => {
                    let images =
                        brushstroke.gen_images_for_last_segments(n_segments, image_scale)?;

                    match images {
                        GeneratedStrokeImages::Partial(mut images) => {
                            let mut rendernodes = render::Image::images_to_rendernodes(&images)?;

                            render_comp.rendernodes.append(&mut rendernodes);
                            render_comp.images.append(&mut images);
                            render_comp.regenerate_flag = true;
                        }
                        GeneratedStrokeImages::Full(mut images) => {
                            let mut rendernodes = render::Image::images_to_rendernodes(&images)?;

                            render_comp.rendernodes.append(&mut rendernodes);
                            render_comp.images.append(&mut images);
                            render_comp.regenerate_flag = false;
                        }
                    }
                }
                // regenerate everything for strokes that don't support generating svgs for the last added elements
                Stroke::ShapeStroke(_) | Stroke::VectorImage(_) | Stroke::BitmapImage(_) => {
                    self.regenerate_rendering_for_stroke_threaded(key, viewport, image_scale);
                }
            }
        }
        Ok(())
    }

    /// generates images and appends them to the render component for the last segments of brushstrokes. For other strokes all rendering is regenerated
    #[allow(unused)]
    pub fn append_rendering_last_segments_threaded(
        &mut self,
        key: StrokeKey,
        n_segments: usize,
        viewport: AABB,
        image_scale: f64,
    ) -> anyhow::Result<()> {
        if let Some(stroke) = self.strokes.get(key) {
            let tasks_tx = self.tasks_tx.clone();
            let stroke = stroke.clone();

            // Spawn a new thread for image rendering
            self.threadpool.spawn(move || {
                match stroke {
                    Stroke::BrushStroke(brushstroke) => {
                        match brushstroke.gen_images_for_last_segments(n_segments, image_scale) {
                            Ok(images) => {
                                tasks_tx.unbounded_send(StoreTask::AppendImagesToStroke {
                                    key,
                                    images,
                                }).unwrap_or_else(|e| {
                                    log::error!("tasks_tx.send() AppendImagesToStroke failed in append_rendering_last_segments_threaded() for stroke with key {:?}, with Err, {}",key, e);
                                });
                            }
                            Err(e) => {
                                log::error!("tasks_tx.send() AppendImagesToStroke failed in append_rendering_last_segments_threaded() for stroke with key {:?}, with Err, {}",key, e);
                            }
                        }
                    }
                    // regenerate everything for strokes that don't support generating svgs for the last added elements
                    Stroke::ShapeStroke(_) | Stroke::VectorImage(_) | Stroke::BitmapImage(_) => {
                        match stroke.gen_images(viewport, image_scale) {
                            Ok(images) => {
                                tasks_tx.unbounded_send(StoreTask::UpdateStrokeWithImages {
                                    key,
                                    images,
                                }).unwrap_or_else(|e| {
                                    log::error!("tasks_tx.send() UpdateStrokeWithImages failed in append_rendering_last_segments_threaded() for stroke with key {:?}, with Err, {}",key, e);
                                });
                            }
                            Err(e) => {
                                log::debug!("stroke.gen_image() failed in append_rendering_last_segments_threaded() for stroke with key {:?}, with Err {}", key, e);
                            }
                        }
                    }
                }
            });
        }
        Ok(())
    }

    /// Not changing the regenerate flag, that is the responsibility of the caller
    pub fn replace_rendering_with_images(
        &mut self,
        key: StrokeKey,
        images: GeneratedStrokeImages,
    ) -> anyhow::Result<()> {
        if let Some(render_comp) = self.render_components.get_mut(key) {
            match images {
                GeneratedStrokeImages::Partial(images) => {
                    let rendernodes = render::Image::images_to_rendernodes(&images)?;
                    render_comp.rendernodes = rendernodes;
                    render_comp.images = images;
                    render_comp.regenerate_flag = true;
                }
                GeneratedStrokeImages::Full(images) => {
                    let rendernodes = render::Image::images_to_rendernodes(&images)?;
                    render_comp.rendernodes = rendernodes;
                    render_comp.images = images;
                    render_comp.regenerate_flag = false;
                }
            }
        }
        Ok(())
    }

    /// Not changing the regenerate flag, that is the responsibility of the caller
    pub fn append_rendering_images(
        &mut self,
        key: StrokeKey,
        images: GeneratedStrokeImages,
    ) -> anyhow::Result<()> {
        if let Some(render_comp) = self.render_components.get_mut(key) {
            match images {
                GeneratedStrokeImages::Partial(mut images) => {
                    let mut rendernodes = render::Image::images_to_rendernodes(&images)?;

                    render_comp.rendernodes.append(&mut rendernodes);
                    render_comp.images.append(&mut images);
                }
                GeneratedStrokeImages::Full(mut images) => {
                    let mut rendernodes = render::Image::images_to_rendernodes(&images)?;
                    render_comp.rendernodes.append(&mut rendernodes);
                    render_comp.images.append(&mut images);
                }
            }
        }
        Ok(())
    }

    /// Draws the strokes without the selection
    pub fn draw_strokes_snapshot(&self, snapshot: &Snapshot, sheet_bounds: AABB, viewport: AABB) {
        snapshot.push_clip(&graphene::Rect::from_p2d_aabb(sheet_bounds));

        self.stroke_keys_as_rendered_intersecting_bounds(viewport)
            .iter()
            .for_each(|&key| {
                if let (Some(stroke), Some(render_comp)) =
                    (self.strokes.get(key), self.render_components.get(key))
                {
                    if render_comp.rendernodes.is_empty() {
                        Self::draw_stroke_placeholder(snapshot, stroke.bounds())
                    }

                    for rendernode in render_comp.rendernodes.iter() {
                        snapshot.append_node(rendernode);
                    }
                }
            });

        snapshot.pop();
    }

    /// Draws the selection
    pub fn draw_selection_snapshot(
        &self,
        snapshot: &Snapshot,
        _sheet_bounds: AABB,
        viewport: AABB,
    ) {
        self.selection_keys_as_rendered_intersecting_bounds(viewport)
            .into_iter()
            .for_each(|key| {
                if let (Some(stroke), Some(render_comp)) =
                    (self.strokes.get(key), self.render_components.get(key))
                {
                    if render_comp.rendernodes.is_empty() {
                        Self::draw_stroke_placeholder(snapshot, stroke.bounds())
                    }

                    for rendernode in render_comp.rendernodes.iter() {
                        snapshot.append_node(rendernode);
                    }
                }
            });
    }

    fn draw_stroke_placeholder(snapshot: &Snapshot, bounds: AABB) {
        snapshot.push_blur(4.0);
        snapshot.append_color(
            &gdk::RGBA::from_piet_color(color::GNOME_BRIGHTS[1].with_a8(0x90)),
            &graphene::Rect::from_p2d_aabb(bounds),
        );
        snapshot.pop();
    }

    pub fn draw_strokes_immediate_w_piet(
        &self,
        piet_cx: &mut impl piet::RenderContext,
        _sheet_bounds: AABB,
        viewport: AABB,
        image_scale: f64,
    ) -> anyhow::Result<()> {
        self.keys_sorted_chrono_intersecting_bounds(viewport)
            .into_iter()
            .for_each(|key| {
                if let Some(stroke) = self.strokes.get(key) {
                    if let Err(e) = || -> anyhow::Result<()> {
                        piet_cx.save().map_err(|e| anyhow::anyhow!("{}", e))?;
                        stroke
                            .draw(piet_cx, image_scale)
                            .map_err(|e| anyhow::anyhow!("{}", e))?;
                        piet_cx.restore().map_err(|e| anyhow::anyhow!("{}", e))?;
                        Ok(())
                    }() {
                        log::error!(
                            "drawing stroke in draw_strokes_immediate_w_piet() failed with Err {}",
                            e
                        );
                    }
                }
            });

        Ok(())
    }

    pub fn draw_selection_immediate_w_piet(
        &self,
        piet_cx: &mut impl piet::RenderContext,
        _sheet_bounds: AABB,
        viewport: AABB,
        image_scale: f64,
    ) -> anyhow::Result<()> {
        self.selection_keys_as_rendered_intersecting_bounds(viewport)
            .into_iter()
            .for_each(|key| {
                if let Some(stroke) = self.strokes.get(key) {
                    if let Err(e) = || -> anyhow::Result<()> {
                        piet_cx.save().map_err(|e| anyhow::anyhow!("{}", e))?;
                        stroke
                            .draw(piet_cx, image_scale)
                            .map_err(|e| anyhow::anyhow!("{}", e))?;
                        piet_cx.restore().map_err(|e| anyhow::anyhow!("{}", e))?;
                        Ok(())
                    }() {
                        log::error!(
                            "drawing stroke in draw_selection_immediate_w_piet() failed with Err {}",
                            e
                        );
                    }
                }
            });

        Ok(())
    }

    pub fn draw_debug(&self, snapshot: &Snapshot, border_widths: f64) {
        self.keys_sorted_chrono().into_iter().for_each(|key| {
            if let Some(stroke) = self.strokes.get(key) {
                // Push blur and opacity for strokes which are normally hidden
                if let Some(render_comp) = self.render_components.get(key) {
                    if let Some(trash_comp) = self.trash_components.get(key) {
                        if render_comp.render && trash_comp.trashed {
                            snapshot.push_blur(3.0);
                            snapshot.push_opacity(0.2);
                        }
                    }
                    if render_comp.regenerate_flag {
                        visual_debug::draw_fill(
                            stroke.bounds(),
                            visual_debug::COLOR_STROKE_REGENERATE_FLAG,
                            snapshot,
                        );
                    }

                    render_comp.images.iter().for_each(|image| {
                        visual_debug::draw_bounds(
                            // a little tightened not to overlap with other bounds
                            image.rect.bounds().tightened(2.0 * border_widths),
                            visual_debug::COLOR_IMAGE_BOUNDS,
                            snapshot,
                            border_widths,
                        )
                    });
                }

                for &hitbox_elem in stroke.hitboxes().iter() {
                    visual_debug::draw_bounds(
                        hitbox_elem,
                        visual_debug::COLOR_STROKE_HITBOX,
                        snapshot,
                        border_widths,
                    );
                }

                visual_debug::draw_bounds(
                    stroke.bounds(),
                    visual_debug::COLOR_STROKE_BOUNDS,
                    snapshot,
                    border_widths,
                );

                match stroke {
                    // Draw positions for brushstrokes
                    Stroke::BrushStroke(brushstroke) => {
                        for element in brushstroke.path.clone().into_elements().iter() {
                            visual_debug::draw_pos(
                                element.pos,
                                visual_debug::COLOR_POS,
                                snapshot,
                                border_widths * 4.0,
                            )
                        }
                    }
                    _ => {}
                }

                // Pop Blur and opacity for hidden strokes
                if let (Some(render_comp), Some(trash_comp)) = (
                    self.render_components.get(key),
                    self.trash_components.get(key),
                ) {
                    if render_comp.render && trash_comp.trashed {
                        snapshot.pop();
                        snapshot.pop();
                    }
                }
            }
        });
    }
}
