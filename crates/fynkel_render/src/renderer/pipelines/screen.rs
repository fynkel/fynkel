//! The passes over the whole target: the solid scene's depth copied and
//! its colour put back for the see-through draws, the last pass onto a
//! view, and screen effects.

use super::*;

impl Pipelines {
    /// behind.wgsl, reading the kept depth as it's drawn: multisampled,
    /// or with one sample, plain.
    fn behind_shader(&self) -> &wgpu::ShaderModule {
        self.behind_shader.get_or_init(|| {
            let source = include_str!("../../behind.wgsl");
            let source = if self.samples > 1 {
                source.into()
            } else {
                source.replace("texture_multisampled_2d<f32>", "texture_2d<f32>")
            };
            self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("behind.wgsl"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        })
    }

    /// Copies one sample of the solid scene's multisampled depth into the
    /// depth shaders that read what's behind see.
    pub(in crate::renderer) fn copy_depth(&self) -> &wgpu::RenderPipeline {
        self.copy_depth.get_or_init(|| {
            let depth_layout = if self.samples > 1 { &self.msaa_depth_layout } else { &self.image_layout };
            let layout = self.layout("scene depth", &[Some(depth_layout)]);
            self.screen(
                "scene depth",
                &layout,
                self.behind_shader(),
                ("vs_main", "fs_depth"),
                &[],
                Some(depth(DEPTH_FORMAT, true, wgpu::CompareFunction::Always)),
                1,
                None,
            )
        })
    }

    /// Puts the solid scene's resolved colour back into the multisampled
    /// target, every sample, for the see-through draws to go over. The
    /// depth is kept, and left as it is.
    pub(in crate::renderer) fn restore(&self) -> &wgpu::RenderPipeline {
        self.restore.get_or_init(|| {
            let layout = self.layout("what's behind", &[Some(&self.image_layout)]);
            self.screen(
                "what's behind",
                &layout,
                self.behind_shader(),
                ("vs_main", "fs_restore"),
                &[],
                Some(depth(DEPTH_FORMAT, false, wgpu::CompareFunction::Always)),
                self.samples,
                target(self.format, None, wgpu::ColorWrites::ALL),
            )
        })
    }

    /// The last pass's pipeline, onto a view (of an sRGB format), adding
    /// bloom's glow or not.
    pub(in crate::renderer) fn finish(&self, bloom: bool) -> &wgpu::RenderPipeline {
        assert!(self.format.is_srgb(), "a view's format is sRGB");
        match bloom {
            false => self.finish.get_or_init(|| self.make_finish(false)),
            true => self.finish_bloom.get_or_init(|| self.make_finish(true)),
        }
    }

    /// The last pass, onto a view: the scene image and, with `bloom`, its
    /// glow.
    fn make_finish(&self, bloom: bool) -> wgpu::RenderPipeline {
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("finish"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(include_str!("../../neutral.wgsl"), include_str!("../../finish.wgsl")).into(),
            ),
        });
        let groups = [Some(&self.image_layout), Some(&self.bloom_layout)];
        let layout = self.layout("finish", if bloom { &groups } else { &groups[..1] });
        self.screen(
            "finish",
            &layout,
            &shader,
            ("vs_main", if bloom { "fs_bloom" } else { "fs_main" }),
            &[],
            None,
            1,
            target(self.format, None, wgpu::ColorWrites::ALL),
        )
    }

    /// A screen effect's pipeline, onto a view's format, made the first
    /// time.
    pub(in crate::renderer) fn effect(&self, shader: &CustomShader) -> wgpu::RenderPipeline {
        self.kept(&self.effects, shader.hash, || {
            self.make_effect(&shader.name, &shader.wgsl, shader.textures.len(), shader.reads_behind)
        })
    }

    /// [`Pipelines::effect`] when it's ready ([`Pipelines::ready`]).
    pub(in crate::renderer) fn effect_ready(
        self: &Arc<Self>,
        shader: &Arc<CustomShader>,
        wait: bool,
    ) -> Option<wgpu::RenderPipeline> {
        let key = shader.hash;
        let shader = Arc::clone(shader);
        self.ready(wait, Building::Effect(key), made(&self.effects, &key), move |p| p.effect(&shader))
    }

    /// An effect that changes nothing, for one freed since its frame was
    /// prepared.
    pub(in crate::renderer) fn effect_copy(&self) -> wgpu::RenderPipeline {
        const COPY: &str = concat!(
            include_str!("../../effect.wgsl"),
            "@fragment\nfn fs_fy_effect(in: FyOut) -> @location(0) vec4<f32> {\n",
            "    return vec4<f32>(picture_at(vec2<i32>(in.clip.xy)).rgb, 1.0);\n}\n"
        );
        self.kept(&self.effects, 0, || self.make_effect("copy", COPY, 0, false))
    }

    /// An effect's pipeline: the picture in group 0; its `textures` of its
    /// own in group 1, as a material shader's; the solid scene's depth in
    /// group 2 if it reads it (`depth`).
    fn make_effect(&self, name: &str, source: &str, textures: usize, depth: bool) -> wgpu::RenderPipeline {
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let own = textures.checked_sub(1).map(|i| &self.own_textures_layouts[i]);
        let groups = [Some(&self.effect_layout), own, depth.then_some(&self.effect_depth_layout)];
        let used = if depth {
            3
        } else if own.is_some() {
            2
        } else {
            1
        };
        let layout = self.layout("screen effect", &groups[..used]);
        self.screen(
            name,
            &layout,
            &shader,
            ("vs_fy_effect", "fs_fy_effect"),
            &[],
            None,
            1,
            target(self.format, None, wgpu::ColorWrites::ALL),
        )
    }
}
