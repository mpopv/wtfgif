#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

typedef struct {
  uint8_t *bytes;
  size_t byte_len;
  size_t byte_capacity;
} NativeEncodedGif;

extern int32_t wtfgif_reencode_gif_fast(
    const uint8_t *data,
    size_t data_len,
    NativeEncodedGif *encoded);
extern void wtfgif_free_bytes(
    uint8_t *bytes,
    size_t byte_len,
    size_t byte_capacity);

static uint64_t now_nanos(void) {
  struct timespec time;
  clock_gettime(CLOCK_MONOTONIC_RAW, &time);
  return (uint64_t)time.tv_sec * 1000000000ull + (uint64_t)time.tv_nsec;
}

int main(int argc, char **argv) {
  if (argc != 2) {
    fprintf(stderr, "usage: %s GIF\n", argv[0]);
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

  for (size_t iteration = 0; iteration < 5; iteration++) {
    NativeEncodedGif encoded = {0};
    uint64_t start = now_nanos();
    if (!wtfgif_reencode_gif_fast(input, (size_t)input_length, &encoded)) {
      fprintf(stderr, "reencode failed\n");
      return 1;
    }
    uint64_t end = now_nanos();
    printf(
        "%llu%s",
        (unsigned long long)(end - start),
        iteration == 4 ? "\n" : " ");
    wtfgif_free_bytes(
        encoded.bytes,
        encoded.byte_len,
        encoded.byte_capacity);
  }
  free(input);
  return 0;
}
