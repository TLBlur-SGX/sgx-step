CC		  = gcc
AR		  = ar
LD		  = gcc
EDGER		  = sgx_edger8r
SIGNER		  = sgx_sign
INCLUDE           = -I$(SGX_SDK)/include/ -I$(SGX_SDK)/include/tlibc
T_CFLAGS	  = $(CFLAGS) -nostdinc -fvisibility=hidden -fpie -fstack-protector -g -O0
U_CFLAGS	  = $(CFLAGS) -nostdinc -fvisibility=hidden -fpie -fstack-protector -g
AR_FLAGS	  = rcs
OBJECTS		  = encl.o

ifeq ($(SIM), 1)
    LIB_SGX_TRTS      = -lsgx_trts_sim
    LIB_SGX_TSERVICE  = -lsgx_tservice_sim
else
    LIB_SGX_TRTS      = -lsgx_trts
    LIB_SGX_TSERVICE  = -lsgx_tservice
endif

ifeq ($(M32), 1)
	T_CFLAGS += -m32 -msse2 -DM32=1
	U_CFLAGS += -m32 -msse2
	LD_FLAGS  = -m32
else
	LIB_SUFX  = 64
endif

ENCLAVE_LIBS      = $(LIB_SGX_TRTS) -ljpeg
ENCLAVE_LIB_PARTS = -lsgx_tstdc -lsgx_tcrypto $(LIB_SGX_TSERVICE)
ENCLAVE	          = encl
PRIVATE_KEY       = private_key.pem
PUBLIC_KEY        = public_key.pem
KEY_SIZE          = 3072
ENCLAVE_EDL       = $(ENCLAVE).edl
ENCLAVE_CONFIG    = $(ENCLAVE).config.xml
OUTPUT_T          = $(ENCLAVE).so
OUTPUT_T_UNSIG    = $(ENCLAVE).unsigned.so
OUTPUT_U          = lib$(ENCLAVE)_proxy.a
LIB_DIRS          = -L $(SGX_SDK)/lib$(LIB_SUFX)/ -L./jpeg-9e
LD_FLAGS         += -Wl,--no-undefined -nostdlib -nodefaultlibs -nostartfiles \
                    -Wl,--whole-archive -Wl,--start-group $(ENCLAVE_LIBS) -Wl,--end-group \
                    -Wl,--no-whole-archive -Wl,--start-group $(ENCLAVE_LIB_PARTS) -Wl,--end-group \
                    -Wl,-Bstatic -Wl,-Bsymbolic -Wl,--no-undefined \
                    -Wl,-pie,-eenclave_entry -Wl,--export-dynamic  \
                    -Wl,--defsym,__ImageBase=0 -Ttlblur.ld -g
TRUSTED_OBJECTS   = $(ENCLAVE)_t.o
UNTRUSTED_OBJECTS = $(ENCLAVE)_u.o 
TRUSTED_CODE      = $(ENCLAVE)_t.h $(ENCLAVE)_t.c
UNTRUSTED_CODE    = $(ENCLAVE)_u.h $(ENCLAVE)_u.c

#.SILENT:
all: libjpeg $(OUTPUT_T) $(OUTPUT_U)

libjpeg:
	$(MAKE) -C jpeg-9e

$(OUTPUT_T) : $(TRUSTED_OBJECTS) $(OBJECTS) $(PRIVATE_KEY)
	echo "$(INDENT)[LD]  " $(OBJECTS) $(TRUSTED_OBJECTS) $(ENCLAVE_LIBS) $(ENCLAVE_LIBS_PARTS) $(OUTPUT_T_UNSIG)
	$(LD) $(OBJECTS) $(TRUSTED_OBJECTS) $(LD_FLAGS) $(LIB_DIRS) -o $(OUTPUT_T_UNSIG) 
	
	echo "$(INDENT)[SGN]" $(OUTPUT_T_UNSIG)
	$(SIGNER) sign -key $(PRIVATE_KEY) -enclave $(OUTPUT_T_UNSIG) -out $(OUTPUT_T) -config $(ENCLAVE_CONFIG) > /dev/null 2> /dev/null

$(OUTPUT_U) : $(UNTRUSTED_OBJECTS) 
	echo "$(INDENT)[AR]  " $(OUTPUT_U)
	$(AR) $(AR_FLAGS) $(OUTPUT_U) $(UNTRUSTED_OBJECTS) 
	
%_t.o : $(subst .o,.c,$@) edger
	echo "$(INDENT)[CC] " $(subst .o,.c,$@) "(trusted edge)"
	touch $(subst .o,.c,$@)
	$(CC) -c $(INCLUDE) $(T_CFLAGS) $(subst .o,.c,$@)

%_u.o : $(subst .o,.c,$@) edger
	echo "$(INDENT)[CC] " $(subst .o,.c,$@) "(untrusted edge)"
	touch $(subst .o,.c,$@)
	$(CC) -c $(INCLUDE) $(U_CFLAGS) $(subst .o,.c,$@)

%.o : %.c edger
	echo "$(INDENT)[CC] " $< "(core)"
	$(CC) $(INCLUDE) $(T_CFLAGS) -c $<

%.o : %.S
	echo "$(INDENT)[AS] " $< "(core)"
	$(CC) $(INCLUDE) $(T_CFLAGS) -c $< -o $@

edger: $(ENCLAVE).edl
	echo "$(INDENT)[GEN]" $(EDGER) $(ENCLAVE_EDL)
	$(EDGER) $(ENCLAVE_EDL)
	
.PHONY: force_check
force_check:
	true

.PHONY: scrub
scrub: 
	echo "$(INDENT)[RM]  " $(PRIVATE_KEY) $(PUBLIC_KEY)
	$(RM) $(PRIVATE_KEY) $(PUBLIC_KEY)

$(PRIVATE_KEY): 
	echo "$(INDENT)[GEN] $(PRIVATE_KEY) ($(KEY_SIZE) bits)"

	# generate 3072 bit private RSA key
	openssl genrsa -out $(PRIVATE_KEY) -3 $(KEY_SIZE)
	
	echo "$(INDENT)[EXT] $(PUBLIC_KEY)"
	# extract public key
	openssl rsa -in $(PRIVATE_KEY) -pubout -out $(PUBLIC_KEY) 
	
.PHONY: clean
clean:
	$(MAKE) -C jpeg-9e clean
	echo "$(INDENT)[RM]" $(OBJECTS) $(OUTPUT_T_UNSIG) $(OUTPUT_T) $(OUTPUT_U)
	$(RM) $(OBJECTS) $(OUTPUT_T_UNSIG) $(OUTPUT_T) $(OUTPUT_U)
	echo "$(INDENT)[RM]" $(TRUSTED_OBJECTS) $(UNTRUSTED_OBJECTS) $(TRUSTED_CODE) $(UNTRUSTED_CODE)
	$(RM) $(TRUSTED_OBJECTS) $(UNTRUSTED_OBJECTS) $(TRUSTED_CODE) $(UNTRUSTED_CODE)
