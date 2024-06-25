#ifndef _SIMSTEP_H_
#define _SIMSTEP_H_

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>

#define ENABLE_TF __asm__ volatile("pushf\norl $0x100, (%rsp)\npopf\n")
#define DISABLE_TF __asm__ volatile("pushf\nandl $0xfffffeff, (%rsp)\npopf\n")

void start_single_stepping();
void stop_single_stepping();

#endif
