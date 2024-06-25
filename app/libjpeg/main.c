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

#include <sgx_urts.h>
#include "Enclave/encl_u.h"
#include <sys/mman.h>
#include <signal.h>
#include "libsgxstep/enclave.h"
#include "libsgxstep/debug.h"
#include "libsgxstep/pt.h"
#include "libsgxstep/file.h"

#define GRAYSCALE       1

#if 1
    #define IMG_NAME    "testimg"
    #define IMG_WIDTH   227
    #define IMG_HEIGHT  149

#elif 0
    #define IMG_NAME    "logo"
    #define IMG_WIDTH   1600
    #define IMG_HEIGHT  1399

#elif 0
    #define IMG_NAME    "birds"
    #define IMG_WIDTH   1600
    #define IMG_HEIGHT  1067
    #define ONEPASS     1
#endif

#if GRAYSCALE
    #define IMG_PATH    IMG_NAME "-gray.jpg"
    #define COLORS      1
#else
    #define IMG_PATH    IMG_NAME ".jpg"
    #define COLORS      3
#endif

#define MAX_SIZE        (IMG_WIDTH * IMG_HEIGHT * 3)+100

unsigned char in_buffer[MAX_SIZE] = {0};
unsigned char out_buffer[MAX_SIZE] = {0};

void ocall_print_string(const char *s)
{
    info("enclave says: %s", s);
}

void ocall_print_int(const char *str, int i)
{
    printf("enclave says: ");
    printf(str, i);
    printf("\n");
}

int zero_cnt = 0, max_cnt = 0, cur_block = 0, color = 0;
char reconstructed_buffer[COLORS][MAX_SIZE] = {0};
int block_cntr[COLORS] = {0};
int reconstruct_width = IMG_WIDTH/8 + ((IMG_WIDTH % 8) ? 1 : 0);
int reconstruct_height= IMG_HEIGHT/8 + ((IMG_HEIGHT % 8) ? 1 : 0);

/* XXX PoC of grayscale img reconstruction via artificially inserted explicit
 * ocall leaks for fct start and all-zero paths (need to be replaced with #PF
 * sequences later)
 */
void ocall_idct_islow(void)
{
    int block = block_cntr[color];
    //info("BLOCK %d: complexity = %d", block, zero_cnt);
    reconstructed_buffer[color][block] = zero_cnt;

    if (zero_cnt > max_cnt)
        max_cnt = zero_cnt;

    zero_cnt = 0;
    block = block_cntr[color]++;
    cur_block++;

    /*
     * NOTE: jdcoefct.c:decompress_onepass() processes each color component
     * sequentially, whereas jdcoefct.c:decompress_data() goes row by row.
     */
    #if !ONEPASS
    if (cur_block >= reconstruct_width)
    #endif
    {
        color = (color+1) % COLORS;
        cur_block = 0;
        //info("new color component %d (cur_block=%d; max_cnt = %d)", color, cur_block, max_cnt);
    }
}

void ocall_all_zero(void)
{
    zero_cnt++;
}

void ocall_next_row(void)
{
    
}

/* https://en.wikipedia.org/wiki/Netpbm#File_formats */
void write_bitmap_img(char *basename, char *desc, char *buf, size_t sz,
        size_t width, size_t height, size_t max_cnt, int grayscale)
{
    char header[1024], path[1024];
    int header_size;
    char *magic_nb, *ext;

    magic_nb = grayscale ? "P5"  : "P6";
    header_size = snprintf(header, 1024, "%s %ld %ld %ld\n", magic_nb, width, height, max_cnt);
    ext = grayscale ? "gray.pgm" : "color.ppm";
    snprintf(path, 1024, "%s-%s-%s", basename, desc, ext);

    file_creat(path);
    file_write(path, header, header_size); 
    file_write_offset(path, buf, sz, header_size);
}

int main( int argc, char **argv )
{
    size_t out_sz = 0, in_sz = 0;
    sgx_enclave_id_t eid = 0;

    info("Creating enclave...");
    SGX_ASSERT( sgx_create_enclave( "./Enclave/encl.so", /*debug=*/1,
                                    NULL, NULL, &eid, NULL ) );

    #if 0
    info_event("calling enclave jpeg compression..");
    in_sz = file_read("./Enclave/jpeg-9e/testimg.ppm", in_buffer, MAX_SIZE);
    info("input size = %d (%d x %d)", in_sz, IMG_WIDTH, IMG_HEIGHT);
    SGX_ASSERT( enclave_jpeg_compress(eid, &out_sz, in_buffer, in_sz, IMG_WIDTH, IMG_HEIGHT, out_buffer, MAX_SIZE) );
    info("output size = %d (%d x %d)", out_sz, IMG_WIDTH, IMG_HEIGHT);
    file_write("out.jpeg", out_buffer, out_sz);
    #endif

    info_event("reading input jpg image (%d x %d)", IMG_WIDTH, IMG_HEIGHT);
    in_sz = file_read("img/" IMG_PATH, in_buffer, MAX_SIZE);
    info("input size = %d (%d x %d)", in_sz, IMG_WIDTH, IMG_HEIGHT);

    info_event("calling enclave jpeg decompression..");
    SGX_ASSERT( enclave_jpeg_decompress(eid, &out_sz, in_buffer, in_sz, out_buffer, MAX_SIZE) );
    info("output size = %d (%d x %d)", out_sz, IMG_WIDTH, IMG_HEIGHT);
    write_bitmap_img(IMG_NAME, "out", out_buffer, out_sz, IMG_WIDTH, IMG_HEIGHT, 255, GRAYSCALE);

    info_event("writing reconstructed image (%d x %d)", reconstruct_width, reconstruct_height);

    /* first write out grayscale images for each color component individually */
    char desc[1024];
    for (int i = 0; i < COLORS; i++)
    {
        snprintf(desc, 1024, "reconstruct-channel-%d", i);
        write_bitmap_img(IMG_NAME, desc, reconstructed_buffer[i], block_cntr[i],
            reconstruct_width, reconstruct_height, max_cnt, /*grayscale=*/1);
    }

    /* now optionally write out a combined color image */
    #if !GRAYSCALE
        int dim = (reconstruct_width * reconstruct_height);
        char *buf = malloc(dim*COLORS);
        for (int i = 0; i < dim; i++)
            for (int j = 0; j < COLORS; j++)
                buf[i*COLORS+j] = reconstructed_buffer[j][i];

        write_bitmap_img(IMG_NAME, "reconstruct", buf, dim*COLORS,
            reconstruct_width, reconstruct_height, max_cnt, /*grayscale=*/0);
    #endif

    info("all is well; exiting..");
    SGX_ASSERT( sgx_destroy_enclave( eid ) );
    return 0;
}
