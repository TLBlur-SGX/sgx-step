/*
 *  This file is part of the SGX-Step enclave execution control framework.
 *
 *  Copyright (C) 2017 Jo Van Bulck <jo.vanbulck@cs.kuleuven.be>,
 *                     Raoul Strackx <raoul.strackx@cs.kuleuven.be>
 *
 *  SGX-Step is free software: you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation, either version 3 of the License, or
 *  (at your option) any later version.
 *
 *  SGX-Step is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with SGX-Step. If not, see <http://www.gnu.org/licenses/>.
 */
#include <stdio.h>
#include "jpeg-9e/jpeglib.h"
#include <string.h>
#include <stdlib.h>

#define MIN(x, y) (((x) < (y)) ? (x) : (y))

size_t enclave_jpeg_decompress(
    unsigned char *jpeg_in_buffer, size_t in_sz,
    unsigned char *out_buffer, size_t out_sz)
{
    struct jpeg_decompress_struct cinfo;
    struct jpeg_error_mgr jerr;
    cinfo.err = jpeg_std_error(&jerr);
    jpeg_create_decompress(&cinfo);

    jpeg_mem_src (&cinfo, jpeg_in_buffer, in_sz);

    jpeg_read_header(&cinfo, TRUE);
    jpeg_start_decompress(&cinfo);

    size_t size = (cinfo.output_width * cinfo.output_height * cinfo.output_components);
    if (out_sz < size)
        return -1;

    size_t row_stride = cinfo.output_width * cinfo.output_components;
    while (cinfo.output_scanline < cinfo.output_height) {
        unsigned char *buffer_array[1];
	buffer_array[0] = out_buffer + (cinfo.output_scanline) * row_stride;
        jpeg_read_scanlines(&cinfo, buffer_array, 1);
    }
    jpeg_finish_decompress(&cinfo);
    jpeg_destroy_decompress(&cinfo);

    return size;
}

/* image_buffer points to large array of R,G,B-order data */
size_t enclave_jpeg_compress(
    unsigned char *image_buffer, size_t in_sz, size_t width, size_t height,
    unsigned char *jpeg_out_buffer, size_t out_sz)
{
    /* --- 1. Allocate and initialize a JPEG compression object. */
    struct jpeg_compress_struct cinfo;
    struct jpeg_error_mgr jerr;
    cinfo.err = jpeg_std_error(&jerr);
    jpeg_create_compress(&cinfo);

    /* --- 2. Specify the destination for the compressed data. */
    unsigned long int length = 0;
    unsigned char *output = NULL;
    jpeg_mem_dest(&cinfo, &output, &length);

    /* --- 3. Set parameters for compression, including image size & colorspace. */
    cinfo.image_width = width; /* image width and height, in pixels */
    cinfo.image_height = height;
    cinfo.input_components = 3;	/* # of color components per pixel */
    cinfo.in_color_space = JCS_RGB; /* colorspace of input image */

    jpeg_set_defaults(&cinfo);
    /* Make optional parameter settings here */
    //cinfo.dct_method = JDCT_ISLOW;

    /* --- 4. jpeg_start_compress(...); */
    jpeg_start_compress(&cinfo, TRUE);

    /* --- 5. while (scan lines remain to be written)
	jpeg_write_scanlines(...); */
    /* Code for this step depends heavily on the way that you store the source data.
    example.c shows the following code for the case of a full-size 2-D source
    array containing 3-byte RGB pixels: */

    JSAMPROW row_pointer[1];	/* pointer to a single row */
    int row_stride;		/* physical row width in buffer */
    row_stride = width * 3;	/* JSAMPLEs per row in image_buffer */

    while (cinfo.next_scanline < cinfo.image_height) {
        int idx = cinfo.next_scanline * row_stride;
        row_pointer[0] = &image_buffer[idx % in_sz];
        jpeg_write_scanlines(&cinfo, row_pointer, 1);
    }

    /* --- 6: Finish compression */
    jpeg_finish_compress(&cinfo);

    /* --- 7: release JPEG compression object */
    /* This is an important step since it will release a good deal of memory. */
    jpeg_destroy_compress(&cinfo);

    memcpy(jpeg_out_buffer, output, MIN(out_sz, length));
    free(output);

    return length;
}

unsigned char *in_buffer;
unsigned char *out_buffer;
size_t in_size = 0;
size_t out_size = 0;

int enclave_jpeg_load_image(
    unsigned char *jpeg_in_buffer, size_t in_sz, size_t max_sz)
{
    in_buffer = malloc(in_sz);
    out_buffer = malloc(max_sz);
    if (!in_buffer || !out_buffer)
        return 1;
    memcpy(in_buffer, jpeg_in_buffer, in_sz);
    in_size = in_sz;
    out_size = max_sz;
    return 0;
}

size_t enclave_jpeg_decompress_loaded() {
    return enclave_jpeg_decompress(in_buffer, in_size, out_buffer, out_size);
}

void enclave_jpeg_free_image() {
    free(in_buffer);
    free(out_buffer);
}

