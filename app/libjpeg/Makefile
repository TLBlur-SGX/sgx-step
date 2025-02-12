LIBSGXSTEP_DIR       = ../..
LIBSGXSTEP           = $(LIBSGXSTEP_DIR)/libsgxstep
URTS_LIB_PATH        = $(LIBSGXSTEP_DIR)/linux-sgx/psw/urts/linux

ifeq ($(SGX_SDK),)
    SGX_SDK          = /opt/intel/sgxsdk
endif
export SGX_SDK
ifneq ($(SGX_SDK), /opt/intel/sgxsdk)
    URTS_LD_LIBRARY_PATH = LD_LIBRARY_PATH=$(LIBSGXSTEP_DIR)/linux-sgx/psw/urts/linux
endif

ENCLAVE              = Enclave
SUBDIRS              = $(ENCLAVE) $(LIBSGXSTEP)

CC                   = gcc
AS                   = gcc
LD                   = gcc

ifeq ($(M32), 1)
	CFLAGS   = -m32 -DM32=$(M32)
	LDFLAGS  = -m32
else
	LIB_SUFX = 64
endif

ifndef SIM
   export SIM=0
endif
ifeq ($(SIM), 1)
    LIB_SGX_URTS      = -lsgx_urts_sim 
    LIB_SGX_AESERVICE = -lsgx_uae_service_sim
else
    LIB_SGX_URTS      = -lsgx_urts
    LIB_SGX_AESERVICE = -lsgx_uae_service
endif

CFLAGS              += -fPIC -fno-stack-protector -fno-builtin -fno-jump-tables \
                       -fno-common -Wno-attributes -g -D_GNU_SOURCE -O0
INCLUDE              = -I$(SGX_SDK)/include/ -I$(LIBSGXSTEP_DIR) 
LDFLAGS             += -lsgx-step -lelf -lencl_proxy $(LIB_SGX_URTS) \
                       $(LIB_SGX_AESERVICE) -pthread $(SUBDIRS:%=-L %) -L$(SGX_SDK)/lib$(LIB_SUFX)/ \
                       -L$(LIBSGXSTEP_DIR)/linux-sgx/psw/urts/linux

SOURCES              = main.c
OBJECTS              = $(SOURCES:.c=.o)
IMG                  = $(shell ls img/*.jpg | grep -v gray)
IMG_GRAY             = $(IMG:.jpg=-gray.jpg)
OUTPUT               = app
PROFILER             = profiler-libjpeg.so

BUILDDIRS            = $(SUBDIRS:%=build-%)
CLEANDIRS            = $(SUBDIRS:%=clean-%)

MAKEFLAGS           += --silent

all: $(OUTPUT) $(PROFILER)
	echo Built $< with SIM=$(SIM)
	
run: clean all
	./$(OUTPUT)

debug: clean all
	sudo `which sgx-gdb` ./$(OUTPUT)

$(PROFILER): profiler.o
	echo "$(INDENT)[LD]" $< $(LDFLAGS) -shared -o $(PROFILER) 
	$(LD) $< $(LDFLAGS) -shared -o $(PROFILER) 

$(OUTPUT): $(IMG_GRAY) $(BUILDDIRS) $(OBJECTS)
	echo "$(INDENT)[LD]" $(OBJECTS) $(LIBS) -o $(OUTPUT) 
	$(LD) $(OBJECTS) $(LDFLAGS) -o $(OUTPUT) 

%.o : %.c
	echo "$(INDENT)[CC] " $<
	$(CC) $(CFLAGS) $(INCLUDE) -c $<

%.o : %.S
	echo "$(INDENT)[AS] " $<
	$(AS) $(INCLUDE) -c $< -o $@

img/%-gray.jpg : img/%.jpg
	echo "$(INDENT)[IMG] " $@
	convert $< -type GrayScale $@

clean: $(CLEANDIRS)
	echo "$(INDENT)[RM]" $(OBJECTS) $(OUTPUT) $(PROFILER)
	rm -f $(OBJECTS) $(OUTPUT) $(PROFILER)
	rm -f $(IMG_GRAY)
	rm -f *.ppm *.pgm

$(BUILDDIRS):
	echo "$(INDENT)[===] $(@:build-%=%) [===]"
	$(MAKE) -C $(@:build-%=%) INDENT+="$(INDENT_STEP)" SIM=$(SIM) M32=$(M32) curr-dir=$(curr-dir)/$(@:build-%=%)

$(CLEANDIRS):
	echo "$(INDENT)[===] $(@:clean-%=%) [===]"
	$(MAKE) clean -C $(@:clean-%=%) INDENT+="$(INDENT_STEP)" curr-dir=$(curr-dir)/$(@:build-%=%)
