//! Where the render worker's frames go.

/// A place frames are drawn into: the headless readback, the Apple ring or
/// the Android surface. The render worker owns it and calls it from one
/// thread, once per frame:
///
/// 1. [`Target::begin`] hands out somewhere to draw, or `None` when there's
///    nowhere right now (the worker keeps its scene and tries again).
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
}
