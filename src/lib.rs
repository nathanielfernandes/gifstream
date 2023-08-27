pub mod gif;
use std::time::Duration;

use async_stream::try_stream;
use futures::{Future, Stream};
pub use gif::*;

#[derive(Clone, Copy)]
pub struct GifStream<S, F> {
    interval: Duration,
    frame_delay: u16,
    width: u16,
    height: u16,

    speed: i32,
    pub interlaced: bool,
    pub dispose: DisposalMethod,

    pub state: S,
    generator: F,
}

pub const GIF_HEADERS: [(&'static str, &'static str); 8] = [
    ("Content-Type", "image/gif"),
    ("Content-Transfer-Encoding", "binary"),
    ("Cache-Control", "no-cache"),
    ("Cache-Control", "no-store"),
    ("Cache-Control", "no-transform"),
    ("Expires", "0"),
    // cors
    ("Access-Control-Allow-Origin", "*"),
    ("Access-Control-Allow-Methods", "GET"),
];

pub const MIN_DELAY: u128 = 10; // in ms
pub const MAX_DELAY: u128 = 65535; // in 100ths of a second

impl<S, F> GifStream<S, F> {
    pub fn state(mut self, state: S) -> Self {
        self.state = state;
        self
    }

    pub fn interlaced(mut self, interlaced: bool) -> Self {
        self.interlaced = interlaced;
        self
    }

    pub fn dispose(mut self, dispose: DisposalMethod) -> Self {
        self.dispose = dispose;
        self
    }

    // speed is the speed of the color quantization algorithm
    // speed must be between 1 and 30
    // 1 produces the nicest looking gif (but is slow)
    // 10 is a good balance between quality and speed
    // 30 produces a poor quality gif (but is fast)
    pub fn speed(mut self, speed: i32) -> Self {
        assert!(speed > 0 && speed <= 30, "speed must be between 1 and 30");
        self.speed = speed;
        self
    }
}

impl<S, F, D, E, R> GifStream<S, F>
where
    S: Clone + Send,
    F: Fn(S) -> R,
    R: Future<Output = Result<D, E>> + Send + 'static,
    D: AsRef<[u8]>,
{
    pub fn new(interval: Duration, width: u16, height: u16, state: S, image_generator: F) -> Self {
        let delay = interval.as_millis().max(MIN_DELAY);
        let frame_delay = (delay / 10).min(MAX_DELAY) as u16;

        Self {
            interval,
            frame_delay,
            width,
            height,

            state,
            generator: image_generator,

            speed: 10,
            interlaced: false,
            dispose: DisposalMethod::Keep,
        }
    }

    // default stream, assumes no global palette
    // returns a stream of encoded gif frames
    pub fn stream(self) -> impl Stream<Item = Result<Vec<u8>, E>> {
        try_stream! {
            let mut buf = Vec::new();
            let flags = GifEncoder::global_palette_flags(&[]);
            GifEncoder::write_screen_desc(&mut buf, self.width, self.height, Some(flags));
            GifEncoder::write_color_table(&mut buf, &[]);
            yield buf;

            let mut interval = tokio::time::interval(self.interval);
            loop {
                interval.tick().await;

                let mut buf = Vec::new();

                let data =  (self.generator)(self.state.clone()).await?;
                let frame = Frame::from_rgba(self.width, self.height, data.as_ref(), self.speed);

                GifEncoder::write_frame(
                    &mut buf,
                    &frame,
                    self.frame_delay,
                    self.interlaced,
                    self.dispose,
                );

                yield buf;
            }
        }
    }

    // stream with global palette
    // returns a stream of encoded gif frames
    pub fn stream_with_palette(self, gp: GlobalPalette) -> impl Stream<Item = Result<Vec<u8>, E>> {
        try_stream! {
            let mut buf = Vec::new();
            let flags = GifEncoder::global_palette_flags(gp.palette());
            GifEncoder::write_screen_desc(&mut buf, self.width, self.height, Some(flags));
            GifEncoder::write_color_table(&mut buf, gp.palette());
            yield buf;

            let mut interval = tokio::time::interval(self.interval);
            loop {
                interval.tick().await;

                let mut buf = Vec::new();

                let data =  (self.generator)(self.state.clone()).await?;
                let frame = Frame::with_global_palette_rgba(self.width, self.height, data.as_ref(), &gp);

                GifEncoder::write_frame(
                    &mut buf,
                    &frame,
                    self.frame_delay,
                    self.interlaced,
                    self.dispose,
                );

                yield buf;
            }
        }
    }

    // stream with auto generated global palette, given a number of colors
    pub fn stream_auto_palette(self, n_colors: usize) -> impl Stream<Item = Result<Vec<u8>, E>> {
        try_stream! {
            let mut buf = Vec::new();

            let data = (self.generator)(self.state.clone()).await?;
            let gp = GlobalPalette::new(self.speed, n_colors, data.as_ref());

            let flags = GifEncoder::global_palette_flags(gp.palette());
            GifEncoder::write_screen_desc(&mut buf, self.width, self.height, Some(flags));
            GifEncoder::write_color_table(&mut buf, gp.palette());
            yield buf;

            let mut interval = tokio::time::interval(self.interval);
            loop {
                interval.tick().await;

                let mut buf = Vec::new();

                let data = (self.generator)(self.state.clone()).await?;
                let frame = Frame::with_global_palette_rgba(self.width, self.height, data.as_ref(), &gp);

                GifEncoder::write_frame(
                    &mut buf,
                    &frame,
                    self.frame_delay,
                    self.interlaced,
                    self.dispose,
                );

                yield buf;
            }
        }
    }
}
