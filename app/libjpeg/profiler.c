#include "Enclave/encl_u.h"
#include "libsgxstep/debug.h"
#include "libsgxstep/file.h"
#include "libsgxstep/pf.h"
#include "libsgxstep/simstep.h"
#include <fcntl.h>
#include <sgx_urts.h>
#include <signal.h>
#include <stdio.h>
#include <sys/mman.h>

#define GRAYSCALE 1

#if 1
#define IMG_NAME "testimg"
#define IMG_WIDTH 227
#define IMG_HEIGHT 149

#elif 0
#define IMG_NAME "logo"
#define IMG_WIDTH 1600
#define IMG_HEIGHT 1399

#elif 0
#define IMG_NAME "birds"
#define IMG_WIDTH 1600
#define IMG_HEIGHT 1067
#define ONEPASS 1
#endif

#if GRAYSCALE
#define IMG_PATH IMG_NAME "-gray.jpg"
#define COLORS 1
#else
#define IMG_PATH IMG_NAME ".jpg"
#define COLORS 3
#endif

#define MAX_SIZE (IMG_WIDTH * IMG_HEIGHT * 3) + 100

unsigned char in_buffer[MAX_SIZE] = {0};
unsigned char out_buffer[MAX_SIZE] = {0};

void ocall_idct_islow(void) { printf("hack: explicit leakage\n"); }

void ocall_all_zero(void) { printf("hack: explicit leakage\n"); }

void ocall_print_string(const char *s) { 
    printf("%s", s);
    fflush(stdout);
}

void ocall_print_int(const char *str, int i) {
  printf(str, i);
  fflush(0);
}

int load_image(sgx_enclave_id_t eid, char *image_path, size_t buffer_size,
               size_t max_size) {
  printf("input size: %d, output_size: %d\n", buffer_size, max_size);
  uint8_t *in_buffer = malloc(buffer_size);
  ASSERT(in_buffer);
  size_t in_sz = file_read(image_path, in_buffer, buffer_size);
  info("read image from file");
  int load_res;
  sgx_status_t res =
      enclave_jpeg_load_image(eid, &load_res, in_buffer, in_sz, max_size);
  if (res != SGX_SUCCESS)
    return res;
  info("loaded image into enclave");
  free(in_buffer);
  info("free returned");
  return load_res;
}

int decompress_image(sgx_enclave_id_t eid) {
  info("decompressing image");
  size_t out_size;
  sgx_status_t res = enclave_jpeg_decompress_loaded(eid, &out_size);
  info("decompressing done");
  if (out_size == -1)
    return SGX_ERROR_UNEXPECTED;
  if (res != SGX_SUCCESS)
    return res;
  info("decompressing ok");
  return 0;
}

int free_image(sgx_enclave_id_t eid) {
  info("freeing image");
  sgx_status_t res = enclave_jpeg_free_image(eid);
  if (res != SGX_SUCCESS)
    return res;
  return 0;
}

void profiler_setup(int eid, int e_size, void *e_start, uint64_t argc,
                    char **argv) {
  if (argc != 3)
    exit(1);
  char *image_path = argv[0];
  size_t buffer_size = atoi(argv[1]);
  size_t max_size = atoi(argv[2]);

  SGX_ASSERT(load_image(eid, image_path, buffer_size, max_size));
}

void profiler_run(int eid) {
  info_event("calling enclave jpeg decompression..");
  start_single_stepping();
  SGX_ASSERT(decompress_image(eid));
  stop_single_stepping();
}

void profiler_destroy(int eid) {
  SGX_ASSERT(free_image(eid));
}
