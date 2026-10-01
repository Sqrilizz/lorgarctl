obj-m += hid-lorgar.o

KDIR ?= /lib/modules/$(shell uname -r)/build
PWD := $(shell pwd)
LLVM ?= 1

default:
	$(MAKE) -C $(KDIR) M=$(PWD) LLVM=$(LLVM) modules

clean:
	$(MAKE) -C $(KDIR) M=$(PWD) clean
