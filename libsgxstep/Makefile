-include Makefile.config

CC          = gcc
AS          = gcc
AR          = ar
ARFLAGS     = -rcs
CFLAGS      = -fPIC

ifneq "$(WITH_PSTATE)" "0"
CPPFLAGS     += -DHAS_PSTATE
endif

ifneq "$(WITH_TURBO)" "0"
CPPFLAGS     += -DHAS_TURBO
endif

ifeq ($(M32), 1)
	ASFLAGS = -m32 -DM32=1
	CFLAGS += -m32 -DM32=1
endif

SOURCES     = $(shell ls *.c)
ASM         = $(shell ls *.S)
OBJECTS     = $(SOURCES:.c=.o) $(ASM:.S=.o)
OUTPUT      = libsgx-step.a

ifeq ($(GRAMINE), 1)
	CFLAGS += -DSGX_SSAFRAMESIZE=16384
	SOURCES += ./../sdk/gramine/aep.c
endif

CFLAGS += -fPIC

all: $(OUTPUT)

%.a: $(OBJECTS)
	@echo "$(INDENT)[AR] " $@
	$(AR) $(ARFLAGS) $@ $(OBJECTS)

%.so: $(OBJECTS)
	@echo "$(INDENT)[LD] " $@
	$(CC) -shared $(LDFLAGS) $(OBJECTS) -o $@

%.o : %.c
	@echo "$(INDENT)[CC] " $<
	$(CC) $(CPPFLAGS) $(CFLAGS) $(INCLUDE) -c $< -o $@

%.o : %.S
	@echo "$(INDENT)[AS] " $<
	$(AS) $(ASFLAGS) $(INCLUDE) -c $< -o $@

clean:
	@echo "$(INDENT)[RM]" $(OBJECTS) $(OUTPUT)
	rm -f $(OBJECTS) $(OUTPUT)
