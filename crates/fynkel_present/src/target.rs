//! Where the render worker's frames go.

/// What a target calls when a frame frees its place ([`Target::on_free`]).
pub type Wake = std::sync::Arc<dyn Fn() + Send + Sync>;

/// A place frames are drawn into: the headless readback, the Apple ring or
/// the Android surface. The render worker owns it and calls it from one
/// thread, once per frame:
///
/// 1. [`Target::begin`] hands out somewhere to draw, or `None` when there's
///    nowhere right now. A target that calls back when a slot frees
///    ([`Target::on_free`]) gets the same scene again then, unless a newer
///    one came; others lose the scene and get the next one posted.
/// 2. The worker records the frame into a command encoder, drawing into
///    [`Target::view`].
/// 3. [`Target::finish`] completes the frame however the target needs to:
///    a readback copy, a completion callback, a present. It submits the
///    encoder.
pub trait Target: Send {
    /// One frame in progress.
    type Frame: Send;

    /// The format of the views handed out. Always an sRGB format.
    fn format(&self) -> wgpu::TextureFormat;

    /// The size frames are drawn at, in pixels.
    fn size(&self) -> (u32, u32);

    /// Somewhere to draw the next frame, or `None` for now.
    fn begin(&mut self) -> Option<Self::Frame>;

    /// The view to draw `frame` into.
    fn view<'a>(&'a self, frame: &'a Self::Frame) -> &'a wgpu::TextureView;

    /// Completes `frame`, whose commands are in `encoder`, and submits.
    /// `number` is the frame's number from the caller, for targets that
    /// report which frame they show.
    fn finish(&mut self, frame: Self::Frame, encoder: wgpu::CommandEncoder, number: u64);

    /// Calls `wake` (from any thread) each time a frame finishes and frees
    /// the place it was drawn in, so a scene [`Target::begin`] had no room
    /// for is drawn as soon as there is some. Returns whether it will:
    /// targets whose `begin` fails for other reasons don't.
    fn on_free(&mut self, wake: Wake) -> bool {
        let _ = wake;
        false
    }

    /// Draws later frames at `width` × `height` (each at least 1). Frames
    /// already begun finish at the old size. Fails only when the platform
    /// can't make textures of that size.
    fn resize(&mut self, width: u32, height: u32) -> Result<(), String>;
}
