# gifstream

A simple library for creating a live gif stream, which can be used to send live gifs to a web browser.

inspiration from https://hookrace.net/blog/time.gif/

## Why it works

The gif spec does not specify how many frames are in a gif, so we can send a gif with an infinite number of frames. This library uses a generator function to generate a frame every x milliseconds, and sends it to the client. The client will keep the connection open, and will keep receiving and displaying the frames.

To keep things flowing, each frame will carry a delay which will be the same as the time between frames.

## Example usage with axum

```rust
use gifstream::{GifStream, GIF_HEADERS};

// generic function to generate a frame
// Note: the function can return any type that can be read as &[u8]
// the error can be anything aswell
async fn generate_frame(state: AppState) -> Result<Vec<u8>, &'static str> {
    let img = state.gen_image();
    Ok(img.into_raw())
}

// GET endpoint
async fn live_gif(State(state): State<AppState>) -> impl IntoResponse {
    let headers = GIF_HEADERS; // headers for a gif, exported from the library

    let gs = GifStream::new(
        Duration::from_millis(1000), // how often to generate a frame
        400, // width
        100, // height
        state, // state to pass to generate_frame,
        generate_frame, // function to generate a frame
    );

    let stream = gs.stream(); // create an async stream
    let body = StreamBody::new(stream);

    (headers, body)
}
```

the gif encoder is modified and based off the image crate.
