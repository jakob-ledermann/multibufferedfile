#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef struct FileReader {
  BufferedFileReader<File> *_0;
} FileReader;

typedef struct FileWriter {
  BufferedFileWriter<File> *_0;
} FileWriter;

struct FileReader open_read(const char *path);

struct FileWriter open_write(const char *path);

int64_t read(struct FileReader reader, uint8_t *buffer, uintptr_t buffer_len);

int64_t write(struct FileWriter writer, uint8_t *buffer, uintptr_t buffer_len);

void close_read(struct FileReader reader);

void close_write(struct FileWriter writer);
