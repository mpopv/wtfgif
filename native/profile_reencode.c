#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

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
    NativeEncodedGif encoded = {0};
    if (!wtfgif_reencode_gif_fast(input, (size_t)input_length, &encoded)) {
      fprintf(stderr, "reencode failed\n");
      return 1;
    }
    checksum ^= encoded.bytes[encoded.byte_len - 1];
    wtfgif_free_bytes(encoded.bytes, encoded.byte_len, encoded.byte_capacity);
  }
  free(input);
  printf("checksum=%u\n", checksum);
  return 0;
}
