#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef BufferedFileReader<File> *FileReader;

typedef BufferedFileWriter<File> *FileWriter;

FileReader bufferedfile_open_read(const char *path);

FileWriter bufferedfile_open_write(const char *path);

int64_t bufferedfile_read(FileReader reader, uint8_t *buffer, uintptr_t buffer_len);

int64_t bufferedfile_write(FileWriter writer, uint8_t *buffer, uintptr_t buffer_len);

void bufferedfile_close_read(FileReader reader);

void bufferedfile_close_write(FileWriter writer);
