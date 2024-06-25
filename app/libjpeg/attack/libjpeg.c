#include "encl_u.h"
#include "debug.h"
#include "file.h"
#include "pf.h"
#include "sgx_error.h"
#include <fcntl.h>
#include <sgx_urts.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>

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
