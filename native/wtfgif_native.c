#include <node_api.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#ifdef __APPLE__
#include <dispatch/dispatch.h>
#endif

typedef struct {
  uint8_t *pixels;
  size_t byte_len;
  uint32_t width;
  uint32_t height;
  uint32_t frame_count;
  int32_t host_owned;
} NativeDecodedGif;

typedef struct {
  uint8_t *bytes;
  size_t byte_len;
  size_t byte_capacity;
} NativeEncodedGif;

typedef struct {
  size_t byte_len;
  size_t byte_capacity;
} EncodedBufferHint;

typedef uint8_t *(*NativeRgbaAllocator)(void *context, size_t byte_len);
extern int32_t wtfgif_decode_all_rgba_host(
    const uint8_t *data,
    size_t data_len,
    NativeRgbaAllocator allocate,
    void *allocate_context,
    NativeDecodedGif *decoded);
extern void wtfgif_free_rgba(uint8_t *pixels, size_t byte_len);
extern int32_t wtfgif_encode_rgba_fast(
    const uint8_t *rgba_stream,
    size_t rgba_len,
    uint16_t width,
    uint16_t height,
    size_t frame_count,
    const uint32_t *palette_rgb,
    size_t palette_len,
    const uint16_t *delays,
    size_t delay_count,
    int32_t loop_count,
    int32_t deltas,
    NativeEncodedGif *encoded);
extern int32_t wtfgif_encode_rgba_quality(
    const uint8_t *rgba_stream,
    size_t rgba_len,
    uint16_t width,
    uint16_t height,
    size_t frame_count,
    const uint16_t *delays,
    size_t delay_count,
    int32_t loop_count,
    uint8_t alpha_threshold,
    NativeEncodedGif *encoded);
extern int32_t wtfgif_encode_indexed_fast(
    const uint8_t *index_stream,
    size_t index_len,
    uint16_t width,
    uint16_t height,
    size_t frame_count,
    const uint32_t *palette_rgb,
    size_t palette_len,
    const uint16_t *delays,
    size_t delay_count,
    int32_t loop_count,
    int32_t deltas,
    NativeEncodedGif *encoded);
extern int32_t wtfgif_reencode_gif_fast_host(
    const uint8_t *data,
    size_t data_len,
    NativeRgbaAllocator allocate,
    void *allocate_context);
extern void wtfgif_free_bytes(
    uint8_t *bytes,
    size_t byte_len,
    size_t byte_capacity);

static napi_value throw_error(napi_env env, const char *message) {
  napi_throw_error(env, NULL, message);
  return NULL;
}

static int get_uint8_input(
    napi_env env,
    napi_value value,
    void **data,
    size_t *length,
    const char *message);

static void finalize_rgba(napi_env env, void *data, void *finalize_hint) {
  (void)env;
  wtfgif_free_rgba((uint8_t *)data, (size_t)finalize_hint);
}

static void finalize_bytes(napi_env env, void *data, void *finalize_hint) {
  (void)env;
  EncodedBufferHint *hint = (EncodedBufferHint *)finalize_hint;
  wtfgif_free_bytes(
      (uint8_t *)data,
      hint->byte_len,
      hint->byte_capacity);
  free(hint);
}

static napi_value expose_encoded_buffer(
    napi_env env,
    NativeEncodedGif encoded) {
  return expose_encoded_buffer(env, encoded);
}

typedef struct {
  napi_env env;
  napi_value buffer;
} HostRgbaBuffer;

static uint8_t *allocate_host_rgba(void *context, size_t byte_len) {
  HostRgbaBuffer *host = (HostRgbaBuffer *)context;
  void *data = NULL;
  if (napi_create_buffer(
          host->env,
          byte_len,
          &data,
          &host->buffer) != napi_ok) {
    return NULL;
  }
  return (uint8_t *)data;
}

typedef struct {
  uint16_t x;
  uint16_t y;
  uint16_t width;
  uint16_t height;
  uint16_t delay;
  uint8_t disposal;
  uint8_t transparent;
  uint8_t min_code_size;
  bool has_transparency;
  size_t palette_offset;
  size_t palette_size;
  size_t data_offset;
} TinyGifFrame;

static uint16_t read_le16(const uint8_t *bytes) {
  return (uint16_t)(bytes[0] | ((uint16_t)bytes[1] << 8));
}

static bool skip_gif_sub_blocks(
    const uint8_t *input,
    size_t input_length,
    size_t *offset) {
  while (*offset < input_length) {
    size_t length = input[(*offset)++];
    if (length == 0) {
      return true;
    }
    if (length > input_length - *offset) {
      return false;
    }
    *offset += length;
  }
  return false;
}

__attribute__((noinline))
static bool decode_tiny_lzw_256(
    const uint8_t *input,
    size_t input_length,
    size_t input_offset,
    uint8_t min_code_size,
    uint8_t *indices,
    size_t pixel_count) {
  uint16_t string_start[4096];
  uint16_t string_length[4096];
  size_t output_offset = 0;
  uint64_t bits = 0;
  unsigned bit_count = 0;
  unsigned clear = 1u << min_code_size;
  unsigned eoi = clear + 1;
  unsigned code_size = min_code_size + 1;
  unsigned code_mask = (1u << code_size) - 1;
  unsigned next_code = eoi + 1;
  size_t previous_start = 0;
  size_t previous_length = 0;
  uint8_t previous_first = 0;
  bool have_previous = false;
  if (input_offset >= input_length) {
    return false;
  }
  size_t block_remaining = input[input_offset++];
  if (block_remaining == 0 || block_remaining > input_length - input_offset) {
    return false;
  }

  for (;;) {
    if (bit_count < code_size && block_remaining >= sizeof(uint32_t)) {
      uint32_t word;
      memcpy(&word, input + input_offset, sizeof(word));
      bits |= (uint64_t)word << bit_count;
      bit_count += 32;
      input_offset += sizeof(word);
      block_remaining -= sizeof(word);
    }
    while (bit_count < code_size) {
      if (block_remaining == 0) {
        if (input_offset >= input_length) {
          return false;
        }
        block_remaining = input[input_offset++];
        if (block_remaining == 0 ||
            block_remaining > input_length - input_offset) {
          return false;
        }
      }
      bits |= (uint64_t)input[input_offset++] << bit_count;
      bit_count += 8;
      block_remaining--;
    }
    unsigned code = (unsigned)bits & code_mask;
    bits >>= code_size;
    bit_count -= code_size;
    if (code == clear) {
      code_size = min_code_size + 1;
      code_mask = (1u << code_size) - 1;
      next_code = eoi + 1;
      have_previous = false;
      continue;
    }
    if (code == eoi) {
      return output_offset == pixel_count;
    }

    size_t current_start = output_offset;
    size_t decoded_length;
    uint8_t first;
    if (code < clear) {
      if (output_offset >= pixel_count) {
        return false;
      }
      first = (uint8_t)code;
      indices[output_offset++] = first;
      decoded_length = 1;
    } else {
      size_t source_start;
      size_t source_length;
      bool append_first = code == next_code;
      if (code > next_code || (append_first && !have_previous)) {
        return false;
      }
      if (append_first) {
        source_start = previous_start;
        source_length = previous_length;
        first = previous_first;
      } else {
        source_start = string_start[code];
        source_length = string_length[code];
        first = indices[source_start];
      }
      decoded_length = source_length + (append_first ? 1 : 0);
      if (source_start + source_length > output_offset ||
          decoded_length > pixel_count - output_offset) {
        return false;
      }
      if (source_length <= 64) {
        uint64_t words[8];
        memcpy(words, indices + source_start, sizeof(words));
        memcpy(indices + output_offset, words, sizeof(words));
      } else {
        memcpy(
            indices + output_offset,
            indices + source_start,
            source_length);
      }
      output_offset += source_length;
      if (append_first) {
        indices[output_offset++] = first;
      }
    }

    if (have_previous && next_code < 4096) {
      string_start[next_code] = (uint16_t)previous_start;
      string_length[next_code] = (uint16_t)(previous_length + 1);
      next_code++;
      if (next_code >= code_mask + 1 && code_size < 12) {
        code_size++;
        code_mask = (1u << code_size) - 1;
      }
    }
    previous_start = current_start;
    previous_length = decoded_length;
    previous_first = first;
    have_previous = true;
  }
}

static size_t tiny_literal_lzw_size(size_t pixel_count) {
  size_t code_count = pixel_count + (pixel_count + 253) / 254 + 1;
  size_t compressed_length = (code_count * 9 + 7) / 8;
  return 1 + compressed_length + (compressed_length + 254) / 255 + 1;
}

__attribute__((noinline))
static size_t encode_tiny_literal_lzw_256(
    uint8_t *output,
    const uint8_t *indices,
    size_t pixel_count,
    uint8_t *compressed) {
  size_t compressed_length = 0;
  unsigned __int128 bits = 0;
  unsigned bit_count = 0;
  size_t index = 0;
  while (index < pixel_count) {
    bits |= (unsigned __int128)256 << bit_count;
    bit_count += 9;
    while (bit_count >= 8) {
      compressed[compressed_length++] = (uint8_t)bits;
      bits >>= 8;
      bit_count -= 8;
    }
    size_t end = index + 254;
    if (end > pixel_count) {
      end = pixel_count;
    }
    while (index + 8 <= end) {
      unsigned __int128 packed =
          (unsigned __int128)indices[index] |
          ((unsigned __int128)indices[index + 1] << 9) |
          ((unsigned __int128)indices[index + 2] << 18) |
          ((unsigned __int128)indices[index + 3] << 27) |
          ((unsigned __int128)indices[index + 4] << 36) |
          ((unsigned __int128)indices[index + 5] << 45) |
          ((unsigned __int128)indices[index + 6] << 54) |
          ((unsigned __int128)indices[index + 7] << 63);
      bits |= packed << bit_count;
      bit_count += 72;
      uint64_t low = (uint64_t)bits;
      memcpy(compressed + compressed_length, &low, sizeof(low));
      compressed[compressed_length + 8] = (uint8_t)(bits >> 64);
      compressed_length += 9;
      bits >>= 72;
      bit_count -= 72;
      index += 8;
    }
    while (index < end) {
      bits |= (unsigned __int128)indices[index++] << bit_count;
      bit_count += 9;
      while (bit_count >= 8) {
        compressed[compressed_length++] = (uint8_t)bits;
        bits >>= 8;
        bit_count -= 8;
      }
    }
  }
  bits |= (unsigned __int128)257 << bit_count;
  bit_count += 9;
  while (bit_count >= 8) {
    compressed[compressed_length++] = (uint8_t)bits;
    bits >>= 8;
    bit_count -= 8;
  }
  if (bit_count != 0) {
    compressed[compressed_length++] = (uint8_t)bits;
  }

  size_t output_offset = 0;
  output[output_offset++] = 8;
  size_t compressed_offset = 0;
  while (compressed_offset < compressed_length) {
    size_t length = compressed_length - compressed_offset;
    if (length > 255) {
      length = 255;
    }
    output[output_offset++] = (uint8_t)length;
    memcpy(output + output_offset, compressed + compressed_offset, length);
    output_offset += length;
    compressed_offset += length;
  }
  output[output_offset++] = 0;
  return output_offset;
}

static size_t tiny_frame_output_size(const TinyGifFrame *frame) {
  size_t pixels = (size_t)frame->width * frame->height;
  return
      (frame->delay != 0 || frame->disposal != 0 ||
       frame->has_transparency ? 8 : 0) +
      10 + tiny_literal_lzw_size(pixels);
}

static bool encode_tiny_frame(
    const uint8_t *input,
    size_t input_length,
    const TinyGifFrame *frame,
    uint8_t *output,
  size_t expected_length) {
  size_t pixels = (size_t)frame->width * frame->height;
  size_t code_count = pixels + (pixels + 253) / 254 + 1;
  size_t compressed_capacity = (code_count * 9 + 7) / 8;
  uint8_t indices[pixels + sizeof(uint64_t) * 8];
  uint8_t compressed_output[compressed_capacity];
  if (!decode_tiny_lzw_256(
          input,
          input_length,
          frame->data_offset,
          frame->min_code_size,
          indices,
          pixels)) {
    return false;
  }
  size_t output_offset = 0;
  if (frame->delay != 0 || frame->disposal != 0 ||
      frame->has_transparency) {
    output[output_offset++] = 0x21;
    output[output_offset++] = 0xf9;
    output[output_offset++] = 4;
    output[output_offset++] =
        (uint8_t)((frame->disposal << 2) |
                  (frame->has_transparency ? 1 : 0));
    output[output_offset++] = (uint8_t)frame->delay;
    output[output_offset++] = (uint8_t)(frame->delay >> 8);
    output[output_offset++] =
        frame->has_transparency ? frame->transparent : 0;
    output[output_offset++] = 0;
  }
  output[output_offset++] = 0x2c;
  output[output_offset++] = (uint8_t)frame->x;
  output[output_offset++] = (uint8_t)(frame->x >> 8);
  output[output_offset++] = (uint8_t)frame->y;
  output[output_offset++] = (uint8_t)(frame->y >> 8);
  output[output_offset++] = (uint8_t)frame->width;
  output[output_offset++] = (uint8_t)(frame->width >> 8);
  output[output_offset++] = (uint8_t)frame->height;
  output[output_offset++] = (uint8_t)(frame->height >> 8);
  output[output_offset++] = 0;
  output_offset += encode_tiny_literal_lzw_256(
      output + output_offset, indices, pixels, compressed_output);
  return output_offset == expected_length;
}

#ifdef __APPLE__
typedef struct {
  const uint8_t *input;
  size_t input_length;
  const TinyGifFrame *frames;
  const size_t *frame_offsets;
  const size_t *frame_lengths;
  uint8_t *output;
  _Atomic int failed;
} TinyReencodeDispatchContext;

static void encode_tiny_frame_dispatch(void *opaque, size_t frame_index) {
  TinyReencodeDispatchContext *context =
      (TinyReencodeDispatchContext *)opaque;
  if (!encode_tiny_frame(
          context->input,
          context->input_length,
          &context->frames[frame_index],
          context->output + context->frame_offsets[frame_index],
          context->frame_lengths[frame_index])) {
    atomic_store_explicit(&context->failed, 1, memory_order_relaxed);
  }
}
#endif

static bool try_reencode_tiny_global_256(
    napi_env env,
    const uint8_t *input,
    size_t input_length,
    napi_value *result) {
  if (input_length < 19 || input_length > 65536 ||
      (memcmp(input, "GIF87a", 6) != 0 &&
       memcmp(input, "GIF89a", 6) != 0)) {
    return false;
  }
  bool has_global_palette = (input[10] & 0x80) != 0;
  size_t global_palette_size =
      has_global_palette ? 2u << (input[10] & 7) : 0;
  size_t global_palette_bytes = global_palette_size * 3;
  if (13 + global_palette_bytes > input_length) {
    return false;
  }
  uint16_t canvas_width = read_le16(input + 6);
  uint16_t canvas_height = read_le16(input + 8);
  if (canvas_width == 0 || canvas_height == 0 ||
      (size_t)canvas_width * canvas_height > 16384) {
    return false;
  }

  TinyGifFrame frames[16];
  size_t frame_count = 0;
  size_t total_pixels = 0;
  size_t offset = 13 + global_palette_bytes;
  uint16_t pending_delay = 0;
  uint8_t pending_disposal = 0;
  uint8_t pending_transparent = 0;
  bool pending_has_transparency = false;
  int loop_count = -1;
  while (offset < input_length) {
    uint8_t block = input[offset++];
    if (block == 0x3b) {
      break;
    }
    if (block == 0x21) {
      if (offset >= input_length) {
        return false;
      }
      uint8_t label = input[offset++];
      if (label == 0xf9) {
        if (offset + 6 > input_length || input[offset] != 4 ||
            input[offset + 5] != 0) {
          return false;
        }
        uint8_t packed = input[offset + 1];
        pending_delay = read_le16(input + offset + 2);
        pending_disposal = (packed >> 2) & 7;
        pending_has_transparency = (packed & 1) != 0;
        pending_transparent = input[offset + 4];
        offset += 6;
        continue;
      }
      if (label == 0xff && offset + 12 <= input_length &&
          input[offset] == 11 &&
          memcmp(input + offset + 1, "NETSCAPE2.0", 11) == 0) {
        offset += 12;
        if (offset + 5 > input_length || input[offset] != 3 ||
            input[offset + 1] != 1 || input[offset + 4] != 0) {
          return false;
        }
        loop_count = read_le16(input + offset + 2);
        offset += 5;
        continue;
      }
      if (!skip_gif_sub_blocks(input, input_length, &offset)) {
        return false;
      }
      continue;
    }
    if (block != 0x2c || frame_count == 16 || offset + 10 > input_length) {
      return false;
    }
    TinyGifFrame *frame = &frames[frame_count];
    frame->x = read_le16(input + offset);
    frame->y = read_le16(input + offset + 2);
    frame->width = read_le16(input + offset + 4);
    frame->height = read_le16(input + offset + 6);
    uint8_t packed = input[offset + 8];
    offset += 9;
    if (frame->width == 0 || frame->height == 0 ||
        frame->x + frame->width > canvas_width ||
        frame->y + frame->height > canvas_height ||
        (packed & 0x40) != 0) {
      return false;
    }
    if ((packed & 0x80) != 0) {
      frame->palette_size = 2u << (packed & 7);
      frame->palette_offset = offset;
      size_t palette_bytes = frame->palette_size * 3;
      if (palette_bytes > input_length - offset) {
        return false;
      }
      offset += palette_bytes;
    } else {
      if (!has_global_palette) {
        return false;
      }
      frame->palette_size = global_palette_size;
      frame->palette_offset = 13;
    }
    if (offset >= input_length) {
      return false;
    }
    frame->min_code_size = input[offset++];
    if (frame->min_code_size < 2 || frame->min_code_size > 8) {
      return false;
    }
    size_t pixels = (size_t)frame->width * frame->height;
    if (pixels > 16384 || total_pixels + pixels > 200000) {
      return false;
    }
    total_pixels += pixels;
    frame->delay = pending_delay;
    frame->disposal = pending_disposal;
    frame->transparent = pending_transparent;
    frame->has_transparency = pending_has_transparency;
    frame->data_offset = offset;
    pending_delay = 0;
    pending_disposal = 0;
    pending_transparent = 0;
    pending_has_transparency = false;
    if (!skip_gif_sub_blocks(input, input_length, &offset)) {
      return false;
    }
    frame_count++;
  }
  if (frame_count == 0) {
    return false;
  }
  size_t output_palette_size = frames[0].palette_size;
  size_t output_palette_offset = frames[0].palette_offset;
  size_t output_palette_bytes = output_palette_size * 3;
  for (size_t frame_index = 1; frame_index < frame_count; frame_index++) {
    if (frames[frame_index].palette_size != output_palette_size ||
        (frames[frame_index].palette_offset != output_palette_offset &&
         memcmp(
             input + frames[frame_index].palette_offset,
             input + output_palette_offset,
             output_palette_bytes) != 0)) {
      return false;
    }
  }

  size_t frame_offsets[16];
  size_t frame_lengths[16];
  size_t output_length = 13 + 768 + (loop_count >= 0 ? 19 : 0);
  for (size_t frame_index = 0; frame_index < frame_count; frame_index++) {
    const TinyGifFrame *frame = &frames[frame_index];
    frame_offsets[frame_index] = output_length;
    frame_lengths[frame_index] = tiny_frame_output_size(frame);
    output_length += frame_lengths[frame_index];
  }
  output_length++;

  void *output_data = NULL;
  napi_value arraybuffer;
  if (napi_create_arraybuffer(
          env, output_length, &output_data, &arraybuffer) != napi_ok ||
      napi_create_typedarray(
          env,
          napi_uint8_array,
          output_length,
          arraybuffer,
          0,
          result) != napi_ok) {
    return false;
  }
  uint8_t *output = (uint8_t *)output_data;
  size_t output_offset = 0;
  memcpy(output, "GIF89a", 6);
  output_offset += 6;
  memcpy(output + output_offset, input + 6, 7);
  output[10] = (uint8_t)((input[10] & 0x78) | 0x87);
  output_offset += 7;
  memcpy(
      output + output_offset,
      input + output_palette_offset,
      output_palette_bytes);
  output_offset += output_palette_bytes;
  memset(output + output_offset, 0, 768 - output_palette_bytes);
  output_offset += 768 - output_palette_bytes;
  if (loop_count >= 0) {
    static const uint8_t loop_prefix[16] = {
      0x21, 0xff, 0x0b, 'N', 'E', 'T', 'S', 'C',
      'A', 'P', 'E', '2', '.', '0', 0x03, 0x01
    };
    memcpy(output + output_offset, loop_prefix, sizeof(loop_prefix));
    output_offset += sizeof(loop_prefix);
    output[output_offset++] = (uint8_t)loop_count;
    output[output_offset++] = (uint8_t)(loop_count >> 8);
    output[output_offset++] = 0;
  }

  if (output_offset != frame_offsets[0]) {
    return false;
  }

#ifdef __APPLE__
  if (frame_count >= 8 && total_pixels >= 30000) {
    TinyReencodeDispatchContext context = {
      .input = input,
      .input_length = input_length,
      .frames = frames,
      .frame_offsets = frame_offsets,
      .frame_lengths = frame_lengths,
      .output = output,
      .failed = 0,
    };
    dispatch_apply_f(
        frame_count,
        dispatch_get_global_queue(DISPATCH_QUEUE_PRIORITY_DEFAULT, 0),
        &context,
        encode_tiny_frame_dispatch);
    if (atomic_load_explicit(&context.failed, memory_order_relaxed) != 0) {
      return false;
    }
  } else
#endif
  {
    for (size_t frame_index = 0; frame_index < frame_count; frame_index++) {
      if (!encode_tiny_frame(
              input,
              input_length,
              &frames[frame_index],
              output + frame_offsets[frame_index],
              frame_lengths[frame_index])) {
        return false;
      }
    }
  }
  output[output_length - 1] = 0x3b;
  return true;
}

static napi_value decode_frames_rgba(
    napi_env env,
    napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  if (napi_get_cb_info(env, info, &argc, argv, NULL, NULL) != napi_ok ||
      argc != 1) {
    return throw_error(env, "decodeFramesRgba expects one GIF buffer.");
  }

  void *input_data = NULL;
  size_t input_length = 0;
  if (!get_uint8_input(
          env,
          argv[0],
          &input_data,
          &input_length,
          "GIF input must be a Buffer or Uint8Array.")) {
    return NULL;
  }

  NativeDecodedGif decoded = {0};
  HostRgbaBuffer host = {env, NULL};
  if (!wtfgif_decode_all_rgba_host(
          (const uint8_t *)input_data,
          input_length,
          allocate_host_rgba,
          &host,
          &decoded)) {
    return throw_error(env, "Native decoder could not decode GIF input.");
  }

  napi_value output_buffer;
  if (decoded.host_owned) {
    output_buffer = host.buffer;
  } else if (decoded.byte_len <= 16 * 1024) {
    if (napi_create_buffer_copy(
            env,
            decoded.byte_len,
            decoded.pixels,
            NULL,
            &output_buffer) != napi_ok) {
      wtfgif_free_rgba(decoded.pixels, decoded.byte_len);
      return throw_error(env, "Could not expose decoded frame buffer.");
    }
    wtfgif_free_rgba(decoded.pixels, decoded.byte_len);
  } else if (napi_create_external_buffer(
                 env,
                 decoded.byte_len,
                 decoded.pixels,
                 finalize_rgba,
                 (void *)decoded.byte_len,
                 &output_buffer) != napi_ok) {
      wtfgif_free_rgba(decoded.pixels, decoded.byte_len);
      return throw_error(env, "Could not expose decoded frame buffer.");
  }

  napi_value result;
  napi_value js_width;
  napi_value js_height;
  napi_value js_frame_count;
  result = output_buffer;
  napi_create_uint32(env, decoded.width, &js_width);
  napi_create_uint32(env, decoded.height, &js_height);
  napi_create_uint32(env, decoded.frame_count, &js_frame_count);
  const napi_property_attributes result_attributes =
      napi_writable | napi_enumerable | napi_configurable;
  const napi_property_descriptor result_properties[] = {
      {"width", NULL, NULL, NULL, NULL, js_width, result_attributes, NULL},
      {"height", NULL, NULL, NULL, NULL, js_height, result_attributes, NULL},
      {"frameCount", NULL, NULL, NULL, NULL, js_frame_count, result_attributes, NULL},
      {"pixels", NULL, NULL, NULL, NULL, output_buffer, result_attributes, NULL},
  };
  napi_define_properties(
      env,
      result,
      sizeof(result_properties) / sizeof(result_properties[0]),
      result_properties);
  return result;
}

static int get_uint32(
    napi_env env,
    napi_value value,
    uint32_t *result,
    const char *message) {
  if (napi_get_value_uint32(env, value, result) != napi_ok) {
    throw_error(env, message);
    return 0;
  }
  return 1;
}

static int get_int32(
    napi_env env,
    napi_value value,
    int32_t *result,
    const char *message) {
  if (napi_get_value_int32(env, value, result) != napi_ok) {
    throw_error(env, message);
    return 0;
  }
  return 1;
}

static int get_bool(
    napi_env env,
    napi_value value,
    int32_t *result,
    const char *message) {
  bool boolean;
  if (napi_get_value_bool(env, value, &boolean) != napi_ok) {
    throw_error(env, message);
    return 0;
  }
  *result = boolean ? 1 : 0;
  return 1;
}

static int get_typed_array(
    napi_env env,
    napi_value value,
    napi_typedarray_type expected_type,
    void **data,
    size_t *length,
    const char *message) {
  napi_typedarray_type type;
  napi_value array_buffer;
  size_t byte_offset;
  if (napi_get_typedarray_info(
          env,
          value,
          &type,
          length,
          data,
          &array_buffer,
          &byte_offset) != napi_ok ||
      type != expected_type) {
    throw_error(env, message);
    return 0;
  }
  return 1;
}

static int get_uint8_input(
    napi_env env,
    napi_value value,
    void **data,
    size_t *length,
    const char *message) {
  if (napi_get_buffer_info(env, value, data, length) == napi_ok) {
    return 1;
  }
  return get_typed_array(
      env,
      value,
      napi_uint8_array,
      data,
      length,
      message);
}

static napi_value encode_rgba_fast(
    napi_env env,
    napi_callback_info info) {
  size_t argc = 8;
  napi_value argv[8];
  if (napi_get_cb_info(env, info, &argc, argv, NULL, NULL) != napi_ok ||
      argc != 8) {
    return throw_error(
        env,
        "encodeRgbaFast expects RGBA, width, height, frame count, palette, delays, loop count, and delta mode.");
  }

  void *rgba_data = NULL;
  size_t rgba_length = 0;
  if (!get_uint8_input(
          env,
          argv[0],
          &rgba_data,
          &rgba_length,
          "RGBA input must be a Buffer or Uint8Array.")) {
    return NULL;
  }

  uint32_t width;
  uint32_t height;
  uint32_t frame_count;
  int32_t loop_count;
  int32_t deltas;
  if (!get_uint32(env, argv[1], &width, "Width must be an unsigned integer.") ||
      !get_uint32(env, argv[2], &height, "Height must be an unsigned integer.") ||
      !get_uint32(
          env,
          argv[3],
          &frame_count,
          "Frame count must be an unsigned integer.") ||
      !get_int32(env, argv[6], &loop_count, "Loop count must be an integer.") ||
      !get_bool(env, argv[7], &deltas, "Delta mode must be a boolean.")) {
    return NULL;
  }
  if (width > UINT16_MAX || height > UINT16_MAX) {
    return throw_error(env, "Width and height must fit in 16 bits.");
  }

  void *palette_data = NULL;
  size_t palette_length = 0;
  void *delay_data = NULL;
  size_t delay_length = 0;
  if (!get_typed_array(
          env,
          argv[4],
          napi_uint32_array,
          &palette_data,
          &palette_length,
          "Palette must be a Uint32Array.") ||
      !get_typed_array(
          env,
          argv[5],
          napi_uint16_array,
          &delay_data,
          &delay_length,
          "Delays must be a Uint16Array.")) {
    return NULL;
  }

  NativeEncodedGif encoded = {0};
  if (!wtfgif_encode_rgba_fast(
          (const uint8_t *)rgba_data,
          rgba_length,
          (uint16_t)width,
          (uint16_t)height,
          frame_count,
          (const uint32_t *)palette_data,
          palette_length,
          (const uint16_t *)delay_data,
          delay_length,
          loop_count,
          deltas,
          &encoded)) {
    return throw_error(
        env,
        "Native exact encoder rejected invalid or non-GIF-representable RGBA input.");
  }

  return expose_encoded_buffer(env, encoded);
}

static napi_value encode_rgba_quality(
    napi_env env,
    napi_callback_info info) {
  size_t argc = 7;
  napi_value argv[7];
  if (napi_get_cb_info(env, info, &argc, argv, NULL, NULL) != napi_ok ||
      argc != 7) {
    return throw_error(
        env,
        "encodeRgbaQuality expects RGBA, width, height, frame count, delays, loop count, and alpha threshold.");
  }

  void *rgba_data = NULL;
  size_t rgba_length = 0;
  if (!get_uint8_input(
          env,
          argv[0],
          &rgba_data,
          &rgba_length,
          "RGBA input must be a Buffer or Uint8Array.")) {
    return NULL;
  }

  uint32_t width;
  uint32_t height;
  uint32_t frame_count;
  uint32_t alpha_threshold;
  int32_t loop_count;
  if (!get_uint32(env, argv[1], &width, "Width must be an unsigned integer.") ||
      !get_uint32(env, argv[2], &height, "Height must be an unsigned integer.") ||
      !get_uint32(
          env,
          argv[3],
          &frame_count,
          "Frame count must be an unsigned integer.") ||
      !get_int32(env, argv[5], &loop_count, "Loop count must be an integer.") ||
      !get_uint32(
          env,
          argv[6],
          &alpha_threshold,
          "Alpha threshold must be an unsigned integer.")) {
    return NULL;
  }
  if (width > UINT16_MAX || height > UINT16_MAX) {
    return throw_error(env, "Width and height must fit in 16 bits.");
  }
  if (alpha_threshold > UINT8_MAX) {
    return throw_error(env, "Alpha threshold must fit in 8 bits.");
  }

  void *delay_data = NULL;
  size_t delay_length = 0;
  if (!get_typed_array(
          env,
          argv[4],
          napi_uint16_array,
          &delay_data,
          &delay_length,
          "Delays must be a Uint16Array.")) {
    return NULL;
  }

  NativeEncodedGif encoded = {0};
  if (!wtfgif_encode_rgba_quality(
          (const uint8_t *)rgba_data,
          rgba_length,
          (uint16_t)width,
          (uint16_t)height,
          frame_count,
          (const uint16_t *)delay_data,
          delay_length,
          loop_count,
          (uint8_t)alpha_threshold,
          &encoded)) {
    return throw_error(
        env,
        "Native quality encoder rejected invalid RGBA input.");
  }

  return expose_encoded_buffer(env, encoded);
}

static napi_value encode_indexed_fast(
    napi_env env,
    napi_callback_info info) {
  size_t argc = 8;
  napi_value argv[8];
  if (napi_get_cb_info(env, info, &argc, argv, NULL, NULL) != napi_ok ||
      argc != 8) {
    return throw_error(
        env,
        "encodeIndexedFast expects indices, width, height, frame count, palette, delays, loop count, and delta mode.");
  }

  void *index_data = NULL;
  size_t index_length = 0;
  if (!get_uint8_input(
          env,
          argv[0],
          &index_data,
          &index_length,
          "Indexed input must be a Buffer or Uint8Array.")) {
    return NULL;
  }

  uint32_t width;
  uint32_t height;
  uint32_t frame_count;
  int32_t loop_count;
  int32_t deltas;
  if (!get_uint32(env, argv[1], &width, "Width must be an unsigned integer.") ||
      !get_uint32(env, argv[2], &height, "Height must be an unsigned integer.") ||
      !get_uint32(
          env,
          argv[3],
          &frame_count,
          "Frame count must be an unsigned integer.") ||
      !get_int32(env, argv[6], &loop_count, "Loop count must be an integer.") ||
      !get_bool(env, argv[7], &deltas, "Delta mode must be a boolean.")) {
    return NULL;
  }
  if (width > UINT16_MAX || height > UINT16_MAX) {
    return throw_error(env, "Width and height must fit in 16 bits.");
  }

  void *palette_data = NULL;
  size_t palette_length = 0;
  void *delay_data = NULL;
  size_t delay_length = 0;
  if (!get_typed_array(
          env,
          argv[4],
          napi_uint32_array,
          &palette_data,
          &palette_length,
          "Palette must be a Uint32Array.") ||
      !get_typed_array(
          env,
          argv[5],
          napi_uint16_array,
          &delay_data,
          &delay_length,
          "Delays must be a Uint16Array.")) {
    return NULL;
  }

  NativeEncodedGif encoded = {0};
  if (!wtfgif_encode_indexed_fast(
          (const uint8_t *)index_data,
          index_length,
          (uint16_t)width,
          (uint16_t)height,
          frame_count,
          (const uint32_t *)palette_data,
          palette_length,
          (const uint16_t *)delay_data,
          delay_length,
          loop_count,
          deltas,
          &encoded)) {
    return throw_error(env, "Native exact encoder rejected indexed input.");
  }

  return expose_encoded_buffer(env, encoded);
}

static napi_value reencode_gif_fast(
    napi_env env,
    napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  if (napi_get_cb_info(env, info, &argc, argv, NULL, NULL) != napi_ok ||
      argc != 1) {
    return throw_error(
        env,
        "reencodeGifFast expects one GIF byte array.");
  }

  void *input_data = NULL;
  size_t input_length = 0;
  if (!get_uint8_input(
          env,
          argv[0],
          &input_data,
          &input_length,
          "GIF input must be a Buffer or Uint8Array.")) {
    return NULL;
  }

  napi_value tiny_output;
  if (try_reencode_tiny_global_256(
          env,
          (const uint8_t *)input_data,
          input_length,
          &tiny_output)) {
    return tiny_output;
  }

  HostRgbaBuffer host = {env, NULL};
  int32_t reencode_status = wtfgif_reencode_gif_fast_host(
          (const uint8_t *)input_data,
          input_length,
          allocate_host_rgba,
          &host);
  if (!reencode_status) {
    return throw_error(env, "Native exact GIF transcoder rejected the input.");
  }
  return host.buffer;
}

static napi_value initialize(napi_env env, napi_value exports) {
  napi_value function;
  napi_create_function(
      env,
      "decodeFramesRgba",
      NAPI_AUTO_LENGTH,
      decode_frames_rgba,
      NULL,
      &function);
  napi_set_named_property(env, exports, "decodeFramesRgba", function);
  napi_create_function(
      env,
      "encodeRgbaFast",
      NAPI_AUTO_LENGTH,
      encode_rgba_fast,
      NULL,
      &function);
  napi_set_named_property(env, exports, "encodeRgbaFast", function);
  napi_create_function(
      env,
      "encodeRgbaQuality",
      NAPI_AUTO_LENGTH,
      encode_rgba_quality,
      NULL,
      &function);
  napi_set_named_property(env, exports, "encodeRgbaQuality", function);
  napi_create_function(
      env,
      "encodeIndexedFast",
      NAPI_AUTO_LENGTH,
      encode_indexed_fast,
      NULL,
      &function);
  napi_set_named_property(env, exports, "encodeIndexedFast", function);
  napi_create_function(
      env,
      "reencodeGifFast",
      NAPI_AUTO_LENGTH,
      reencode_gif_fast,
      NULL,
      &function);
  napi_set_named_property(env, exports, "reencodeGifFast", function);
  return exports;
}

NAPI_MODULE(NODE_GYP_MODULE_NAME, initialize)
