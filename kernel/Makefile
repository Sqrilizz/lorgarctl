obj-m += hid-lorgar.o

KDIR ?= /lib/modules/$(shell uname -r)/build
PWD := $(shell pwd)
LLVM ?= 1

default:
	$(MAKE) -C $(KDIR) M=$(PWD) LLVM=$(LLVM) modules

install: default
	mkdir -p /lib/modules/$(shell uname -r)/extra
	cp hid-lorgar.ko /lib/modules/$(shell uname -r)/extra/
	depmod -a
	@echo "Module hid-lorgar installed to /lib/modules/$(shell uname -r)/extra and depmod updated."

uninstall:
	rm -f /lib/modules/$(shell uname -r)/extra/hid-lorgar.ko
	depmod -a
	@echo "Module hid-lorgar uninstalled."

clean:
	$(MAKE) -C $(KDIR) M=$(PWD) clean

