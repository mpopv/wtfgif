//! The `WtfGifCore` decoder object exported to JavaScript.

use super::*;

#[wasm_bindgen]
impl WtfGifCore {
    #[wasm_bindgen(constructor)]
    pub fn new(data: &[u8]) -> Result<WtfGifCore, JsValue> {
        let metadata = parse_metadata(data).map_err(|message| js_error(&message))?;
        Ok(WtfGifCore {
            data: data.to_vec(),
            metadata,
            decode_scratch: std::cell::RefCell::new(FrameDecodeScratch::default()),
        })
    }

    pub fn width(&self) -> u16 {
        self.metadata.width
    }

    pub fn height(&self) -> u16 {
        self.metadata.height
    }

    pub fn frame_count(&self) -> usize {
        self.metadata.frames.len()
    }

    pub fn metadata_json(&self) -> String {
        self.metadata.to_json()
    }

    pub fn decode_frame_indices(&self, frame_index: usize) -> Result<Vec<u8>, JsValue> {
        let frame = self
            .metadata
            .frames
            .get(frame_index)
            .ok_or_else(|| js_error("Frame index out of range"))?;
        decode_frame_indices_inner(&self.data, frame).map_err(|message| js_error(&message))
    }

    pub fn decode_frame_rgba(&self, frame_index: usize) -> Result<Vec<u8>, JsValue> {
        decode_frame_pixels_inner(&self.data, &self.metadata, frame_index, PixelFormat::Rgba)
            .map_err(|message| js_error(&message))
    }

    pub fn decode_frame_bgra(&self, frame_index: usize) -> Result<Vec<u8>, JsValue> {
        decode_frame_pixels_inner(&self.data, &self.metadata, frame_index, PixelFormat::Bgra)
            .map_err(|message| js_error(&message))
    }

    pub fn decode_and_blit_frame_rgba(
        &self,
        frame_index: usize,
        pixels: &mut [u8],
    ) -> Result<(), JsValue> {
        decode_and_blit_frame_reusing_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Rgba,
            pixels,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| js_error(&message))
    }

    pub fn decode_and_blit_frame_bgra(
        &self,
        frame_index: usize,
        pixels: &mut [u8],
    ) -> Result<(), JsValue> {
        decode_and_blit_frame_reusing_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Bgra,
            pixels,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| js_error(&message))
    }

    /// Decode a common opaque full-canvas frame into reusable Wasm-owned
    /// storage. JavaScript can copy the returned range out without passing its
    /// caller-owned canvas into Wasm first, avoiding an otherwise unavoidable
    /// input copy for frames that overwrite every pixel.
    pub fn decode_frame_rgba_scratch(&self, frame_index: usize) -> Result<usize, JsValue> {
        decode_frame_to_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Rgba,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| js_error(&message))
    }

    pub fn decode_frame_bgra_scratch(&self, frame_index: usize) -> Result<usize, JsValue> {
        decode_frame_to_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Bgra,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| js_error(&message))
    }

    /// Decode one frame's rectangle into reusable RGBA/BGRA scratch storage.
    /// Transparent pixels are returned as zero and must be left untouched by
    /// the caller when overlaying the rectangle onto an existing canvas.
    pub fn decode_frame_rect_rgba_scratch(&self, frame_index: usize) -> Result<usize, JsValue> {
        decode_frame_rect_to_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Rgba,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| js_error(&message))
    }

    pub fn decode_frame_rect_bgra_scratch(&self, frame_index: usize) -> Result<usize, JsValue> {
        decode_frame_rect_to_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Bgra,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| js_error(&message))
    }

    pub fn decode_scratch_ptr(&self) -> usize {
        self.decode_scratch.borrow().output.as_ptr() as usize
    }

    pub fn decode_all_rgba(&self) -> Result<Vec<u32>, JsValue> {
        prepare_all_composited_frames_inner(&self.data, &self.metadata, PixelFormat::Rgba)
            .map_err(|message| js_error(&message))
    }

    pub fn reencode_gif_pixel_perfect(&self) -> Result<Vec<u8>, JsValue> {
        let loop_count = self.metadata.loop_count.map(i32::from).unwrap_or(-1);
        let total_frame_pixels = self
            .metadata
            .frames
            .iter()
            .try_fold(0usize, |total, frame| {
                usize::from(frame.width)
                    .checked_mul(usize::from(frame.height))
                    .and_then(|pixels| total.checked_add(pixels))
                    .ok_or_else(|| "Decoded frame size overflow".to_string())
            });
        let total_frame_pixels = total_frame_pixels.map_err(|message| js_error(&message))?;
        reencode_gif_literal_sequential(&self.data, &self.metadata, loop_count, total_frame_pixels)
            .map_err(|message| js_error(&message))
    }

    pub fn prepare_composited_rgba(&self, requested_frames: &[u8]) -> Result<Vec<u32>, JsValue> {
        prepare_composited_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Rgba,
        )
        .map_err(|message| js_error(&message))
    }

    pub fn prepare_composited_bgra(&self, requested_frames: &[u8]) -> Result<Vec<u32>, JsValue> {
        prepare_composited_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Bgra,
        )
        .map_err(|message| js_error(&message))
    }

    /// Prepare composited frames in Wasm-owned scratch storage. The returned
    /// length refers to `composited_scratch_ptr()` and avoids copying the
    /// complete animation into JavaScript when a caller will consume frames
    /// one at a time.
    pub fn prepare_composited_rgba_scratch(
        &self,
        requested_frames: &[u8],
    ) -> Result<usize, JsValue> {
        let prepared = prepare_composited_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Rgba,
        )
        .map_err(|message| js_error(&message))?;
        let length = prepared.len();
        self.decode_scratch.borrow_mut().composited_output = prepared;
        Ok(length)
    }

    pub fn prepare_composited_bgra_scratch(
        &self,
        requested_frames: &[u8],
    ) -> Result<usize, JsValue> {
        let prepared = prepare_composited_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Bgra,
        )
        .map_err(|message| js_error(&message))?;
        let length = prepared.len();
        self.decode_scratch.borrow_mut().composited_output = prepared;
        Ok(length)
    }

    pub fn composited_scratch_ptr(&self) -> usize {
        self.decode_scratch.borrow().composited_output.as_ptr() as usize
    }

    pub fn prepare_composited_delta_rgba(
        &self,
        requested_frames: &[u8],
    ) -> Result<Vec<u32>, JsValue> {
        prepare_composited_delta_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Rgba,
        )
        .map_err(|message| js_error(&message))
    }

    pub fn prepare_composited_delta_bgra(
        &self,
        requested_frames: &[u8],
    ) -> Result<Vec<u32>, JsValue> {
        prepare_composited_delta_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Bgra,
        )
        .map_err(|message| js_error(&message))
    }

    /// Prepare composited delta frames in Wasm-owned scratch storage. The
    /// returned length refers to `composited_scratch_ptr()` and avoids copying
    /// the delta stream through a wasm-bindgen return value.
    pub fn prepare_composited_delta_rgba_scratch(
        &self,
        requested_frames: &[u8],
    ) -> Result<usize, JsValue> {
        let prepared = prepare_composited_delta_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Rgba,
        )
        .map_err(|message| js_error(&message))?;
        let length = prepared.len();
        self.decode_scratch.borrow_mut().composited_output = prepared;
        Ok(length)
    }

    pub fn prepare_composited_delta_bgra_scratch(
        &self,
        requested_frames: &[u8],
    ) -> Result<usize, JsValue> {
        let prepared = prepare_composited_delta_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Bgra,
        )
        .map_err(|message| js_error(&message))?;
        let length = prepared.len();
        self.decode_scratch.borrow_mut().composited_output = prepared;
        Ok(length)
    }
}
