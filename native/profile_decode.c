#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
  uint8_t *pixels;
  size_t byte_len;
  uint32_t width;
  uint32_t height;
  uint32_t frame_count;
  uint64_t parse_nanos;
  uint64_t decode_nanos;
  uint64_t compose_nanos;
  int32_t host_owned;
} NativeDecodedGif;

extern int32_t wtfgif_decode_all_rgba(
    const uint8_t *data,
    size_t data_len,
    NativeDecodedGif *decoded);
extern void wtfgif_free_rgba(uint8_t *pixels, size_t byte_len);

int main(int argc, char **argv) {
  if (argc != 3) {
    fprintf(stderr, "usage: %s GIF ITERATIONS\n", argv[0]);
    return 2;
  }
  FILE *file = fopen(argv[1], "rb");
  if (file == NULL) {
    perror("fopen");
    return 2;
  }
  fseek(file, 0, SEEK_END);
  long input_length = ftell(file);
  rewind(file);
  uint8_t *input = malloc((size_t)input_length);
  if (input == NULL ||
      fread(input, 1, (size_t)input_length, file) != (size_t)input_length) {
    fprintf(stderr, "could not read GIF\n");
    return 2;
  }
  fclose(file);

  unsigned long iterations = strtoul(argv[2], NULL, 10);
  unsigned int checksum = 0;
  for (unsigned long iteration = 0; iteration < iterations; iteration++) {
    NativeDecodedGif decoded = {0};
    if (!wtfgif_decode_all_rgba(input, (size_t)input_length, &decoded)) {
      fprintf(stderr, "decode failed\n");
      return 1;
    }
    checksum ^= decoded.pixels[decoded.byte_len - 1];
    wtfgif_free_rgba(decoded.pixels, decoded.byte_len);
  }
  free(input);
  printf("checksum=%u\n", checksum);
  return 0;
}
